use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::spanned::Spanned;
use syn::{
    DeriveInput, Fields, ItemStruct, Meta, Token, parse_macro_input, punctuated::Punctuated,
};

/// Convert a snake_case identifier to PascalCase for associated type names.
fn to_assoc_ty_name(ident: &syn::Ident) -> syn::Ident {
    let s = ident.to_string();
    let mut result = String::with_capacity(s.len());
    let mut capitalize = true;
    for ch in s.chars() {
        if ch == '_' {
            capitalize = true;
        } else if capitalize {
            result.push(ch.to_ascii_uppercase());
            capitalize = false;
        } else {
            result.push(ch);
        }
    }
    format_ident!("{}", result)
}

fn repr_info(input: &DeriveInput) -> Result<(bool, bool), syn::Error> {
    let mut has_c = false;
    let mut align16 = false;
    for attr in &input.attrs {
        if !attr.path().is_ident("repr") {
            continue;
        }
        let Meta::List(list) = &attr.meta else {
            continue;
        };
        let parsed: Punctuated<Meta, Token![,]> =
            list.parse_args_with(Punctuated::parse_terminated)?;
        for meta in parsed {
            match meta {
                Meta::Path(path) if path.is_ident("C") => has_c = true,
                Meta::List(list) if list.path.is_ident("align") => {
                    let literal: syn::LitInt = list.parse_args()?;
                    align16 |= literal.base10_parse::<u64>()? == 16;
                }
                _ => {}
            }
        }
    }
    Ok((has_c, align16))
}

fn expand(input: &DeriveInput) -> Result<proc_macro2::TokenStream, syn::Error> {
    let (has_c, is_instance) = repr_info(input)?;
    if !has_c {
        return Err(syn::Error::new(
            input.span(),
            "expected #[repr(C)] on a GpuStruct struct",
        ));
    }
    if !input.generics.params.is_empty() {
        return Err(syn::Error::new(
            input.generics.span(),
            "GpuStruct does not support generic structs",
        ));
    }
    let fields = match &input.data {
        syn::Data::Struct(data) => match &data.fields {
            Fields::Named(named) => &named.named,
            _ => {
                return Err(syn::Error::new(
                    input.span(),
                    "GpuStruct requires a struct with named fields",
                ));
            }
        },
        _ => {
            return Err(syn::Error::new(
                input.span(),
                "GpuStruct requires a struct with named fields",
            ));
        }
    };

    if fields.is_empty() {
        return Err(syn::Error::new(
            input.span(),
            "GpuStruct requires at least one field",
        ));
    }

    let name = &input.ident;
    let twin = format_ident!("{}Gpu", name);
    let bind_group = format_ident!("{}BindGroup", name);
    let resources = format_ident!("{}Resources", bind_group);
    let vis = &input.vis;
    let n = fields.len();
    let n_minus_1 = n - 1;
    let indexes: Vec<usize> = (0..n).collect();

    let field_names: Vec<_> = fields
        .iter()
        .map(|field| field.ident.as_ref().unwrap())
        .collect();
    let gpu_fields: Vec<_> = fields
        .iter()
        .map(|field| {
            let name = field.ident.as_ref().unwrap();
            let ty = &field.ty;
            quote!(#[allow(missing_docs)] pub #name: <#ty as ::shame_gui::GpuStruct>::Gpu)
        })
        .collect();

    // ── {Name}Like trait generation ───────────────────────────────────────
    let trait_name = format_ident!("{}Like", name);
    let assoc_names: Vec<_> = field_names.iter().map(|n| to_assoc_ty_name(n)).collect();
    let cpu_field_tys: Vec<_> = fields.iter().map(|f| &f.ty).collect();
    let gpu_field_tys: Vec<_> = fields
        .iter()
        .map(|field| {
            let ty = &field.ty;
            quote!(<#ty as ::shame_gui::GpuStruct>::Gpu)
        })
        .collect();
    let wgsl_sizes: Vec<_> = fields
        .iter()
        .map(|field| {
            let ty = &field.ty;
            quote!(<#ty as ::shame_gui::GpuStruct>::SIZE as usize)
        })
        .collect();
    let wgsl_aligns: Vec<_> = fields
        .iter()
        .map(|field| {
            let ty = &field.ty;
            quote!(<#ty as ::shame_gui::GpuStruct>::ALIGN as usize)
        })
        .collect();

    let offsets_const = format_ident!("{}_OFFSETS", name.to_string().to_uppercase());
    let byte_size_const = format_ident!("{}_BYTE_SIZE", name.to_string().to_uppercase());

    // ── instance-mode extras ──────────────────────────────────────────────
    let alignment_check = if is_instance {
        quote! {
            const ALIGNS: [usize; #n] = [#(#wgsl_aligns),*];
            let mut max_align: usize = 0;
            let mut i: usize = 0;
            while i < #n {
                if ALIGNS[i] > max_align {
                    max_align = ALIGNS[i];
                }
                i += 1;
            }
            assert!(
                max_align >= 16,
                concat!(
                    "`", stringify!(#name),
                    "` needs at least one 16-byte-aligned field (Vec4, [f32; 4] or [f32; 16])",
                ),
            );
        }
    } else {
        quote! {}
    };

    let size_check = quote! {
        assert!(
            ::core::mem::size_of::<#name>() == #byte_size_const,
            concat!(
                "`", stringify!(#name),
                "`'s Rust size does not match its wire size",
            ),
        );
    };

    // ── GpuStruct impl ────────────────────────────────────────────────────

    let gpu_struct_impl = if is_instance {
        // Instance mode: multi-field serialize with padding, real make_bindings.
        // Gpu = raw GPU twin (bind_group! wraps it in Struct).
        quote! {
            impl ::shame_gui::GpuStruct for #name {
                type Gpu = #twin;
                const SIZE: u64 = #byte_size_const as u64;
                const ALIGN: u64 = 16;

                fn wire_size() -> usize {
                    #byte_size_const
                }

                fn serialize(&self, out: &mut Vec<u8>) {
                    #(
                        if out.len() < #offsets_const[#indexes] {
                            out.resize(#offsets_const[#indexes], 0);
                        }
                        out.extend_from_slice(::shame_gui::bytemuck::bytes_of(&self.#field_names));
                    )*
                    let target = ((out.len() + #byte_size_const - 1) / #byte_size_const) * #byte_size_const;
                    if out.len() < target {
                        out.resize(target, 0);
                    }
                }

                fn make_bindings(gpu: &::shame_gui::sm::Gpu) -> ::shame_gui::material::MaterialBindings {
                    use ::shame_gui::sm::bind_group::AsBindGroupLayout;
                    let result = <#bind_group as AsBindGroupLayout>::create_bind_group_layout(gpu).unwrap();
                    let make_bind_group: ::shame_gui::material::MakeBindGroupFn =
                        |device, layout, buffer| {
                            <#bind_group as AsBindGroupLayout>::create_bind_group(
                                layout,
                                device,
                                #resources {
                                    instances: buffer.clone(),
                                },
                            )
                        };
                    ::shame_gui::material::MaterialBindings {
                        layout: result.inner().clone(),
                        make_bind_group,
                    }
                }
            }
        }
    } else {
        // Field mode: Gpu = sm::Struct<#twin> (the GpuType wrapper).
        quote! {
            impl ::shame_gui::GpuStruct for #name {
                type Gpu = ::shame_gui::sm::Struct<#twin>;
                const SIZE: u64 = #byte_size_const as u64;
                const ALIGN: u64 = {
                    let aligns: [u64; #n] = [#(#wgsl_aligns as u64),*];
                    let mut m: u64 = 0;
                    let mut i: usize = 0;
                    while i < #n {
                        if aligns[i] > m { m = aligns[i]; }
                        i += 1;
                    }
                    m
                };

                fn wire_size() -> usize {
                    #byte_size_const
                }

                fn serialize(&self, out: &mut Vec<u8>) {
                    out.extend_from_slice(::shame_gui::bytemuck::bytes_of(self));
                }

                fn make_bindings(_gpu: &::shame_gui::sm::Gpu) -> ::shame_gui::material::MaterialBindings {
                    unreachable!("field type, not an instance struct")
                }
            }
        }
    };

    // ── bind group (instance mode only) ───────────────────────────────────

    let bind_group_item = if is_instance {
        quote! {
            /// Bind group for `#name` instance storage. Generated by
            /// `#[derive(GpuStruct)]`; constructed by the canvas.
            #[allow(missing_docs)]
            ::shame_gui::sm::bind_group! {
                #vis struct #bind_group {
                    pub instances: ::shame_gui::sm::Buffer<
                        ::shame_gui::sm::Array<::shame_gui::sm::Struct<#twin>>,
                        ::shame_gui::sm::mem::Storage,
                    >,
                }
            }
        }
    } else {
        quote! {}
    };

    let twin_vis = quote! { #vis };

    // Wire size: instance structs (align(16)) round to 16; field structs
    // round to their max member alignment (the WGSL struct size).
    let byte_size_block = if is_instance {
        quote! {
            const #byte_size_const: usize = {
                const SIZES: [usize; #n] = [#(#wgsl_sizes),*];
                (#offsets_const[#n_minus_1] + SIZES[#n_minus_1]).div_ceil(16) * 16
            };
        }
    } else {
        quote! {
            const #byte_size_const: usize = {
                const SIZES: [usize; #n] = [#(#wgsl_sizes),*];
                const ALIGNS: [usize; #n] = [#(#wgsl_aligns),*];
                let mut max_align: usize = 0;
                let mut i: usize = 0;
                while i < #n {
                    if ALIGNS[i] > max_align {
                        max_align = ALIGNS[i];
                    }
                    i += 1;
                }
                (#offsets_const[#n_minus_1] + SIZES[#n_minus_1]).div_ceil(max_align) * max_align
            };
        }
    };

    Ok(quote! {
        /// The GPU twin of `#name`: shame EDSL types mirroring the CPU
        /// struct's WGSL layout. Generated by `#[derive(GpuStruct)]`.
        #[allow(missing_docs)]
        #[derive(::shame_gui::sm::GpuLayout)]
        #[shame_crate(::shame_gui::sm)]
        #[cpu(#name)]
        #twin_vis struct #twin {
            #(#gpu_fields,)*
        }

        // Wire layout, computed at compile time from the fields' `GpuStruct`
        // consts. `offset_of!` gives the actual rustc offsets; the asserts below
        // fail the build if the C layout doesn't match the WGSL layout.
        const #offsets_const: [usize; #n] = ::shame_gui::instance::field_offsets(
            [#(#wgsl_sizes),*],
            [#(#wgsl_aligns),*],
        );

        #byte_size_block

        const _: () = {
            const WGSL: [usize; #n] = #offsets_const;
            const CPU: [usize; #n] = [
                #(::core::mem::offset_of!(#name, #field_names)),*
            ];
            #(
                assert!(
                    WGSL[#indexes] == CPU[#indexes],
                    concat!(
                        "field `", stringify!(#field_names), "` of `", stringify!(#name),
                        "` sits at a different CPU offset than its WGSL offset — reorder fields or insert padding so the C layout matches the WGSL layout",
                    ),
                );
            )*
            #alignment_check
            #size_check
        };

        #bind_group_item

        unsafe impl ::shame_gui::bytemuck::Pod for #name {}
        unsafe impl ::shame_gui::bytemuck::Zeroable for #name {}

        impl ::shame_gui::sm::CpuLayout for #name {
            fn cpu_layout() -> ::shame_gui::sm::TypeLayout {
                <#twin as ::shame_gui::sm::GpuLayout>::gpu_layout()
            }
        }

        #gpu_struct_impl

        // ── {Name}Like<const GPU: bool> ──────────────────────────────────

        #[allow(missing_docs)] #vis trait #trait_name<const GPU: bool> {
            #(type #assoc_names: Clone;)*

            /// Construct from field values.
            fn new(#(#field_names: Self::#assoc_names),*) -> Self;

            #(
                /// Getter for the `#field_names` field (returns by-value clone).
                fn #field_names(&self) -> Self::#assoc_names;
            )*
        }

        /// CPU impl: `{Name}Like<false>` for the host-side struct.
        #[allow(missing_docs)]
        impl #trait_name<false> for #name {
            #(type #assoc_names = #cpu_field_tys;)*

            fn new(#(#field_names: Self::#assoc_names),*) -> Self {
                #name { #(#field_names),* }
            }
            #(
                fn #field_names(&self) -> Self::#assoc_names { self.#field_names.clone() }
            )*
        }

        /// GPU impl: `{Name}Like<true>` for the shader-side twin.
        #[allow(missing_docs)]
        impl #trait_name<true> for #twin {
            #(type #assoc_names = #gpu_field_tys;)*

            fn new(#(#field_names: Self::#assoc_names),*) -> Self {
                #twin { #(#field_names),* }
            }
            #(
                fn #field_names(&self) -> Self::#assoc_names { self.#field_names.clone() }
            )*
        }
    })
}

#[proc_macro_derive(GpuStruct)]
pub fn derive_gpu_struct(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    match expand(&input) {
        Ok(tokens) => tokens.into(),
        Err(error) => error.to_compile_error().into(),
    }
}

/// Derives viewport construction for a named-field state struct.
///
/// **This derive does NOT generate a `Widget` trait impl.** Instead it
/// generates inherent methods that turn the state's port group into viewport
/// tree nodes, one per field:
///
/// - default (no attribute): `into_viewport_nodes(ports) ->
///   Vec<(String, ViewportNode<Self>)>` — **table** mode, each field is a row
///   of `[label | editor]`, the field name being the label;
/// - `#[widget(tab)]`: `into_tab_nodes(ports) ->
///   Vec<(String, Vec<(String, ViewportNode<Self>)>)>` — **tabs** mode.
///
/// # Requirements
///
/// - Must also derive [`DagStruct`](crate::DagStruct) on the same struct
///   (like `Copy: Clone`): the generated code unconditionally references
///   `<Self as DagStruct>::Ports`.
/// - Every field type must implement
///   [`Widget<Self>`](shame_gui::gui::Widget) (primitives implement
///   `Widget<S>` for `Port<T, S>` generically).
#[proc_macro_derive(Widget, attributes(widget))]
pub fn derive_widget(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    match expand_widget(&input) {
        Ok(tokens) => tokens.into(),
        Err(error) => error.to_compile_error().into(),
    }
}

/// Parses `#[widget(tab)]` / `#[widget(skip)]` field attributes.
fn widget_is_tab(input: &DeriveInput) -> bool {
    input.attrs.iter().any(|attr| {
        if !attr.path().is_ident("widget") {
            return false;
        }
        let Meta::List(list) = &attr.meta else {
            return false;
        };
        let parsed: Punctuated<Meta, Token![,]> = list
            .parse_args_with(Punctuated::parse_terminated)
            .unwrap_or_default();
        parsed
            .iter()
            .any(|meta| matches!(meta, Meta::Path(path) if path.is_ident("tab")))
    })
}

/// True if the field is marked `#[widget(skip)]` (not rendered as a widget).
fn field_is_skipped(field: &syn::Field) -> bool {
    field.attrs.iter().any(|attr| {
        if !attr.path().is_ident("widget") {
            return false;
        }
        let Meta::List(list) = &attr.meta else {
            return false;
        };
        let parsed: Punctuated<Meta, Token![,]> = list
            .parse_args_with(Punctuated::parse_terminated)
            .unwrap_or_default();
        parsed
            .iter()
            .any(|meta| matches!(meta, Meta::Path(path) if path.is_ident("skip")))
    })
}

/// True if the field's type is the built-in [`BuiltinState`], which the
/// [`state`](macro@state) attribute macro injects. Such a field holds the
/// framework's source/render state and is never rendered as a widget.
fn field_type_is_builtin_state(field: &syn::Field) -> bool {
    let syn::Type::Path(type_path) = &field.ty else {
        return false;
    };
    type_path
        .path
        .segments
        .last()
        .is_some_and(|seg| seg.ident == "BuiltinState")
}

fn expand_widget(input: &DeriveInput) -> Result<proc_macro2::TokenStream, syn::Error> {
    if !input.generics.params.is_empty() {
        return Err(syn::Error::new(
            input.generics.span(),
            "Widget does not support generic structs",
        ));
    }
    let fields = match &input.data {
        syn::Data::Struct(data) => match &data.fields {
            Fields::Named(named) => &named.named,
            _ => {
                return Err(syn::Error::new(
                    input.span(),
                    "Widget requires a struct with named fields",
                ));
            }
        },
        _ => {
            return Err(syn::Error::new(
                input.span(),
                "Widget requires a struct with named fields",
            ));
        }
    };

    let name = &input.ident;
    let is_tab = widget_is_tab(input);

    // Only fields not marked `#[widget(skip)]` and not the injected built-in
    // state become widgets.
    let rendered_fields: Vec<_> = fields
        .iter()
        .filter(|f| !field_is_skipped(f) && !field_type_is_builtin_state(f))
        .collect();

    let node_exprs: Vec<_> = rendered_fields
        .iter()
        .map(|field| {
            let fname = field.ident.as_ref().unwrap();
            let ty = &field.ty;
            quote! {
                ::shame_gui::gui::ViewportNode::Widget(
                    ::shame_gui::gui::WidgetNode::new(
                        ports.#fname,
                        <::shame_gui::graph::Port<#ty, #name> as ::shame_gui::gui::Widget<#name>>::Data::default(),
                    )
                )
            }
        })
        .collect();

    let pairs: Vec<_> = rendered_fields
        .iter()
        .zip(node_exprs.iter())
        .map(|(field, node)| {
            let label = field.ident.as_ref().unwrap().to_string();
            quote! { (#label.into(), #node) }
        })
        .collect();

    let output = if is_tab {
        let tab_entries: Vec<_> = rendered_fields
            .iter()
            .zip(node_exprs.iter())
            .map(|(field, node)| {
                let label = field.ident.as_ref().unwrap().to_string();
                quote! {
                    (#label.into(), ::std::vec![(#label.into(), #node)])
                }
            })
            .collect();
        quote! {
            impl #name {
                pub fn into_tab_nodes(
                ) -> ::std::vec::Vec<(
                    ::std::string::String,
                    ::std::vec::Vec<(::std::string::String, ::shame_gui::gui::ViewportNode<#name>)>,
                )> {
                    let ports = <#name as ::shame_gui::graph::DagStruct>::ports();
                    ::std::vec![#(#tab_entries),*]
                }
            }
        }
    } else {
        quote! {
            impl #name {
                pub fn into_viewport_nodes(
                ) -> ::std::vec::Vec<(
                    ::std::string::String,
                    ::shame_gui::gui::ViewportNode<#name>,
                )> {
                    let ports = <#name as ::shame_gui::graph::DagStruct>::ports();
                    ::std::vec![#(#pairs),*]
                }
            }
        }
    };

    Ok(output)
}

/// Derives `DagStruct` for a named-field struct: generates a `{Name}Ports`
/// struct (one typed `Port<FieldTy, Self>` per field), a `PortGroup<Self>`
/// impl, a `PortValue` impl, and a `DagStruct` impl whose `ports()` wires up
/// the field accessors.
#[proc_macro_derive(DagStruct)]
pub fn derive_dag_struct(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    match expand_dag_struct(&input) {
        Ok(tokens) => tokens.into(),
        Err(error) => error.to_compile_error().into(),
    }
}

fn expand_dag_struct(input: &DeriveInput) -> Result<proc_macro2::TokenStream, syn::Error> {
    if !input.generics.params.is_empty() {
        return Err(syn::Error::new(
            input.generics.span(),
            "DagStruct does not support generic structs",
        ));
    }
    let fields = match &input.data {
        syn::Data::Struct(data) => match &data.fields {
            Fields::Named(named) => &named.named,
            _ => {
                return Err(syn::Error::new(
                    input.span(),
                    "DagStruct requires a struct with named fields",
                ));
            }
        },
        _ => {
            return Err(syn::Error::new(
                input.span(),
                "DagStruct requires a struct with named fields",
            ));
        }
    };

    let name = &input.ident;
    let ports_name = format_ident!("{}Ports", name);
    let vis = &input.vis;

    let field_names: Vec<_> = fields.iter().map(|f| f.ident.as_ref().unwrap()).collect();
    let field_tys: Vec<_> = fields.iter().map(|f| &f.ty).collect();
    let n = field_names.len();

    // {Name}Ports fields: one Port<FieldTy, Self> per field.
    let ports_fields: Vec<_> = field_tys
        .iter()
        .zip(field_names.iter())
        .map(|(ty, fname)| quote!(pub #fname: ::shame_gui::graph::Port<#ty, #name>))
        .collect();

    // extend_ids: push each field's PortId.
    let extend_ids_stmts: Vec<_> = field_names
        .iter()
        .map(|fname| quote!(out.push(self.#fname.id());))
        .collect();

    // port constructors with accessor fns.
    let port_constructors: Vec<_> = field_names
        .iter()
        .zip(field_tys.iter())
        .enumerate()
        .map(|(idx, (fname, ty))| {
            let idx_lit = idx as u64;
            quote! {
                #fname: ::shame_gui::graph::Port::new(
                    ::shame_gui::graph::PortId::new(#idx_lit),
                    |s: &#name| &s.#fname,
                    |s: &mut #name, v: #ty| { s.#fname = v; },
                    |s: &mut #name| &mut s.#fname,
                ),
            }
        })
        .collect();

    Ok(quote! {
        /// Port group for `#name` — one typed port per field, generated by
        /// `#[derive(DagStruct)]`. Implements `PortGroup<#name>`.
        #[allow(missing_docs)]
        #[derive(Clone, Copy)]
        #vis struct #ports_name {
            #(#ports_fields,)*
        }

        impl ::shame_gui::graph::PortGroup<#name> for #ports_name {
            fn leaf_count(&self) -> usize {
                #n
            }
            fn extend_ids(&self, out: &mut Vec<::shame_gui::graph::PortId>) {
                #(#extend_ids_stmts)*
            }
        }

        impl ::shame_gui::graph::PortValue for #name {}

        impl ::shame_gui::graph::DagStruct for #name {
            type Ports = #ports_name;
            fn ports() -> #ports_name {
                #ports_name {
                    #(#port_constructors)*
                }
            }
        }

        impl #name {
            /// Constructs the port group with field accessors wired up.
            /// Inherent (mirrors [`DagStruct::ports`]) so it can be called
            /// without importing the trait.
            #[allow(non_snake_case)]
            pub fn ports() -> #ports_name {
                <#name as ::shame_gui::graph::DagStruct>::ports()
            }
        }
    })
}

/// Marks a struct as a shame-gui app state: injects the built-in
/// source/render fields (a reserved `pub __builtin: BuiltinState` field) and
/// implements [`AppState`](shame_gui::graph::AppState) for it.
///
/// It is an *attribute* macro (not a derive) so it can add the built-in field
/// before any `#[derive(...)]` attributes on the same struct run — the
/// injected field is therefore visible to `DagStruct` (which numbers it) and
/// skipped by `Widget` (which detects it by type).
///
/// ```ignore
/// #[state]
/// #[derive(Clone, Default, DagStruct, Widget)]
/// struct MyState { volume: f32 }
/// ```
#[proc_macro_attribute]
pub fn state(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let item = parse_macro_input!(item as ItemStruct);
    match expand_state(item) {
        Ok(tokens) => tokens.into(),
        Err(error) => error.to_compile_error().into(),
    }
}

fn expand_state(mut item: ItemStruct) -> Result<proc_macro2::TokenStream, syn::Error> {
    if !item.generics.params.is_empty() {
        return Err(syn::Error::new(
            item.generics.span(),
            "state does not support generic structs",
        ));
    }
    let fields = match &mut item.fields {
        Fields::Named(named) => &mut named.named,
        _ => {
            return Err(syn::Error::new(
                item.span(),
                "state requires a struct with named fields",
            ));
        }
    };
    if fields
        .iter()
        .any(|f| f.ident.as_ref().is_some_and(|i| i == "__builtin"))
    {
        return Err(syn::Error::new(
            item.span(),
            "state injects a reserved `__builtin` field; remove any field with that name",
        ));
    }

    // Inject the built-in source/render fields as the first field. The
    // reserved `__builtin` name signals it is framework-owned; `Widget` skips
    // it by type.
    let builtin_field: syn::Field =
        syn::parse_quote! { pub __builtin: ::shame_gui::graph::BuiltinState };
    fields.insert(0, builtin_field);

    let name = &item.ident;
    let app_state_impl = quote! {
        impl ::shame_gui::graph::AppState for #name {
            fn builtins(&self) -> &::shame_gui::graph::BuiltinState {
                &self.__builtin
            }
            fn builtins_mut(&mut self) -> &mut ::shame_gui::graph::BuiltinState {
                &mut self.__builtin
            }
            fn source_ports() -> ::shame_gui::graph::SourcePorts<#name> {
                use ::shame_gui::graph::PortId;
                let b = ::shame_gui::graph::SOURCE_PORT_BASE;
                ::shame_gui::graph::SourcePorts {
                    framebuffer_size: ::shame_gui::graph::Port::new(
                        PortId::new(b),
                        |s: &#name| &s.__builtin.framebuffer_size,
                        |s: &mut #name, v| s.__builtin.framebuffer_size = v,
                        |s: &mut #name| &mut s.__builtin.framebuffer_size,
                    ),
                    mouse_pos: ::shame_gui::graph::Port::new(
                        PortId::new(b + 1),
                        |s: &#name| &s.__builtin.mouse_pos,
                        |s: &mut #name, v| s.__builtin.mouse_pos = v,
                        |s: &mut #name| &mut s.__builtin.mouse_pos,
                    ),
                    mouse_down: ::shame_gui::graph::Port::new(
                        PortId::new(b + 2),
                        |s: &#name| &s.__builtin.mouse_down,
                        |s: &mut #name, v| s.__builtin.mouse_down = v,
                        |s: &mut #name| &mut s.__builtin.mouse_down,
                    ),
                    scroll_delta: ::shame_gui::graph::Port::new(
                        PortId::new(b + 3),
                        |s: &#name| &s.__builtin.scroll_delta,
                        |s: &mut #name, v| s.__builtin.scroll_delta = v,
                        |s: &mut #name| &mut s.__builtin.scroll_delta,
                    ),
                    delta_time: ::shame_gui::graph::Port::new(
                        PortId::new(b + 4),
                        |s: &#name| &s.__builtin.delta_time,
                        |s: &mut #name, v| s.__builtin.delta_time = v,
                        |s: &mut #name| &mut s.__builtin.delta_time,
                    ),
                    elapsed: ::shame_gui::graph::Port::new(
                        PortId::new(b + 5),
                        |s: &#name| &s.__builtin.elapsed,
                        |s: &mut #name, v| s.__builtin.elapsed = v,
                        |s: &mut #name| &mut s.__builtin.elapsed,
                    ),
                }
            }
            fn render_ports() -> ::shame_gui::graph::RenderPorts<#name> {
                use ::shame_gui::graph::PortId;
                let b = ::shame_gui::graph::RENDER_PORT_BASE;
                ::shame_gui::graph::RenderPorts {
                    fills: ::shame_gui::graph::Port::new(
                        PortId::new(b),
                        |s: &#name| &s.__builtin.fills,
                        |s: &mut #name, v| s.__builtin.fills = v,
                        |s: &mut #name| &mut s.__builtin.fills,
                    ),
                    outlines: ::shame_gui::graph::Port::new(
                        PortId::new(b + 1),
                        |s: &#name| &s.__builtin.outlines,
                        |s: &mut #name, v| s.__builtin.outlines = v,
                        |s: &mut #name| &mut s.__builtin.outlines,
                    ),
                    texts: ::shame_gui::graph::Port::new(
                        PortId::new(b + 2),
                        |s: &#name| &s.__builtin.texts,
                        |s: &mut #name, v| s.__builtin.texts = v,
                        |s: &mut #name| &mut s.__builtin.texts,
                    ),
                }
            }
        }
    };

    Ok(quote! {
        #item
        #app_state_impl
    })
}
