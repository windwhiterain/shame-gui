use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::spanned::Spanned;
use syn::{DeriveInput, Fields, Meta, Token, parse_macro_input, punctuated::Punctuated};

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
                    let target = (out.len() / #byte_size_const + 1) * #byte_size_const;
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
        // The raw GPU twin is not directly a GpuType — it must go through
        // Struct, just like the old InstanceField::Gpu for Rect.
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
/// generates inherent methods that turn the struct into viewport tree
/// nodes, one per field:
///
/// - default (no attribute): `into_viewport_nodes(self, arena, ports) ->
///   Vec<(String, ViewportNode)>` — **table** mode, each field is a row of
///   `[label | editor]`, the field name being the label;
/// - `#[widget(tab)]`: `into_tab_nodes(self, arena, ports) ->
///   Vec<(String, Vec<(String, ViewportNode)>)>` — **tabs** mode, each
///   field is a tab (field name = tab name).
///
/// # Requirements
///
/// - Must also derive [`DagStruct`](crate::DagStruct) on the same struct
///   (like `Copy: Clone`): the generated code unconditionally references
///   `<Self as DagStruct>::Ports`, so a missing `DagStruct` derive fails
///   to compile.
/// - Every field type must implement [`Widget`](shame_gui::gui::Widget)
///   (primitives like `String`, `u32`, `bool`, `f32`, a composite type
///   with its own `#[derive(Widget)]`, ...).
///
/// # `ports` parameter semantics
///
/// - `Some(ports)`: the given `{Name}Ports` are wired into each field's
///   `WidgetNode` via `with_port_ids`, so widget edits mark the shared
///   arena slots dirty and trigger the DAG. Initial values are seeded
///   into the arena via `DagStruct::write_ports`.
/// - `None`: each field allocates standalone arena slots; edits do not
///   feed any DAG node.
#[proc_macro_derive(Widget, attributes(widget))]
pub fn derive_widget(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    match expand_widget(&input) {
        Ok(tokens) => tokens.into(),
        Err(error) => error.to_compile_error().into(),
    }
}

/// Parses `#[widget(tab)]` — the only mode attribute so far; anything else
/// (or nothing) selects the default table mode.
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

/// Returns true if the given derive is also applied to this struct.
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

    // Widget unconditionally references DagStruct::Ports (Copy:Clone pattern).
    // Any struct that derives Widget must also derive DagStruct.
    let ports_type: proc_macro2::TokenStream =
        quote! { <#name as ::shame_gui::graph::DagStruct>::Ports };

    // Build one node expression per field.
    // When ports is Some, seeds the arena and wraps the Port group as widget.
    // When None, allocates standalone arena slots.
    let node_exprs: Vec<_> = fields
        .iter()
        .map(|field| {
            let fname = field.ident.as_ref().unwrap();
            let ty = &field.ty;
            quote! {
                {
                    let mut data = < <#ty as ::shame_gui::graph::DagStruct>::Ports as ::shame_gui::gui::Widget>::Data::default();
                    let (widget_handle, pids) = if let ::std::option::Option::Some(ref ports) = ports {
                        let port_group = ports.#fname.clone();
                        // Seed the arena with the widget's initial value.
                        <#ty as ::shame_gui::graph::DagStruct>::write_ports(
                            &self.#fname, arena, &port_group,
                        );
                        let pids = ::shame_gui::graph::PortGroup::port_ids(&port_group);
                        (Some(port_group), pids)
                    } else {
                        // Standalone: allocate slots via PortGroup::alloc_slots.
                        let port_group = <<#ty as ::shame_gui::graph::DagStruct>::Ports as ::shame_gui::graph::PortGroup>::alloc_slots(arena);
                        <#ty as ::shame_gui::graph::DagStruct>::write_ports(
                            &self.#fname, arena, &port_group,
                        );
                        (Some(port_group), ::std::vec::Vec::new())
                    };
                    let mut node = ::shame_gui::gui::WidgetNode::new(widget_handle.unwrap(), data);
                    if !pids.is_empty() {
                        node = node.with_port_ids(pids);
                    }
                    node
                }
            }
        })
        .collect();

    // Build per-field (label, node) pairs.
    let pairs: Vec<_> = fields
        .iter()
        .zip(node_exprs.iter())
        .map(|(field, node)| {
            let label = field.ident.as_ref().unwrap().to_string();
            quote! { (#label.into(), ::shame_gui::gui::ViewportNode::Widget(#node)) }
        })
        .collect();

    let output = if is_tab {
        // Each field is a tab wrapping a single-node container.
        let tab_entries: Vec<_> = fields
            .iter()
            .zip(node_exprs.iter())
            .map(|(field, node)| {
                let label = field.ident.as_ref().unwrap().to_string();
                quote! {
                    (#label.into(), ::std::vec![(#label.into(), ::shame_gui::gui::ViewportNode::Widget(#node))])
                }
            })
            .collect();
        quote! {
            impl #name {
                pub fn into_tab_nodes(
                    self,
                    arena: &mut ::shame_gui::graph::StateArena,
                    ports: ::std::option::Option<#ports_type>,
                ) -> ::std::vec::Vec<(
                    ::std::string::String,
                    ::std::vec::Vec<(::std::string::String, ::shame_gui::gui::ViewportNode)>,
                )> {
                    ::std::vec![#(#tab_entries),*]
                }
            }
        }
    } else {
        quote! {
            impl #name {
                pub fn into_viewport_nodes(
                    self,
                    arena: &mut ::shame_gui::graph::StateArena,
                    ports: ::std::option::Option<#ports_type>,
                ) -> ::std::vec::Vec<(
                    ::std::string::String,
                    ::shame_gui::gui::ViewportNode,
                )> {
                    ::std::vec![#(#pairs),*]
                }
            }
        }
    };

    Ok(output)
}

/// Derives `DagStruct` for a named-field struct: generates a
/// `{Name}Ports` struct (one typed port per field, recursively for
/// composite DagStruct fields), a `PortGroup` impl, and a `DagStruct`
/// impl mapping the value type to its port group.
///
/// Fields can be primitives (already impl DagStruct with `Ports = Port<T>`)
/// or composite types that also derive DagStruct (e.g. `Vec2` inside `Rect`).
/// Nesting works through `<T as DagStruct>::Ports`.
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

    if fields.is_empty() {
        return Err(syn::Error::new(
            input.span(),
            "DagStruct requires at least one field",
        ));
    }

    let name = &input.ident;
    let ports_name = format_ident!("{}Ports", name);
    let vis = &input.vis;

    let field_names: Vec<_> = fields.iter().map(|f| f.ident.as_ref().unwrap()).collect();
    let field_tys: Vec<_> = fields.iter().map(|f| &f.ty).collect();

    // Generate {Name}Ports struct fields: <T as DagStruct>::Ports
    let ports_fields: Vec<_> = field_tys
        .iter()
        .zip(field_names.iter())
        .map(|(ty, fname)| quote!(pub #fname: <#ty as ::shame_gui::graph::DagStruct>::Ports))
        .collect();

    // PortGroup::leaf_count: sum of children's leaf counts
    let leaf_count_sum: Vec<_> = field_names
        .iter()
        .map(|fname| {
            quote! { ::shame_gui::graph::PortGroup::leaf_count(&self.#fname) }
        })
        .collect();

    // PortGroup::port_ids: collect from each child
    let port_ids_collect: Vec<_> = field_names
        .iter()
        .map(|fname| {
            quote! { ::shame_gui::graph::PortGroup::extend_ids(&self.#fname, &mut v); }
        })
        .collect();

    // PortGroup::extend_ids: same but uses `out` parameter
    let extend_ids_collect: Vec<_> = field_names
        .iter()
        .map(|fname| {
            quote! { ::shame_gui::graph::PortGroup::extend_ids(&self.#fname, out); }
        })
        .collect();

    // PortGroup::set_port_ids: split by each child's leaf_count
    let set_port_ids_stmts: Vec<_> = field_names
        .iter()
        .map(|fname| {
            quote! {
                let n = ::shame_gui::graph::PortGroup::leaf_count(&self.#fname);
                ::shame_gui::graph::PortGroup::set_port_ids(&mut self.#fname, &ids[off..(off + n)]);
                off += n;
            }
        })
        .collect();

    // alloc_slots: allocate arena slots for each field and construct the group
    let alloc_slots_body: Vec<_> = field_names
        .iter()
        .map(|fname| {
            quote! {
                #fname: ::shame_gui::graph::PortGroup::alloc_slots(arena),
            }
        })
        .collect();

    // write_ports: delegate to each field
    let write_ports_body: Vec<_> = field_names
        .iter()
        .zip(field_tys.iter())
        .map(|(fname, ty)| {
            quote! {
                <#ty as ::shame_gui::graph::DagStruct>::write_ports(&self.#fname, arena, &ports.#fname);
            }
        })
        .collect();

    // read_ports: reconstruct the value from its ports (inverse of write_ports)
    let read_ports_body: Vec<_> = field_names
        .iter()
        .zip(field_tys.iter())
        .map(|(fname, ty)| {
            quote! {
                #fname: <#ty as ::shame_gui::graph::DagStruct>::read_ports(arena, &ports.#fname),
            }
        })
        .collect();

    Ok(quote! {
        /// Port group for `#name` — one typed port per field, generated by
        /// `#[derive(DagStruct)]`. Implements `PortGroup` so the whole
        /// struct's ports can be allocated, read, and wired as a unit.
        #[allow(missing_docs)]
        #[derive(Clone, Copy)]
        #vis struct #ports_name {
            #(#ports_fields,)*
        }

        impl ::shame_gui::graph::PortGroup for #ports_name {
            fn leaf_count(&self) -> usize {
                0usize #(+ #leaf_count_sum)*
            }
            fn port_ids(&self) -> Vec<::shame_gui::graph::PortId> {
                let mut v = Vec::new();
                #(#port_ids_collect)*
                v
            }
            fn extend_ids(&self, out: &mut Vec<::shame_gui::graph::PortId>) {
                #(#extend_ids_collect)*
            }
            fn set_port_ids(&mut self, ids: &[::shame_gui::graph::PortId]) {
                let mut off: usize = 0;
                #(#set_port_ids_stmts)*
            }
            fn alloc_slots(arena: &mut ::shame_gui::graph::StateArena) -> Self {
                Self {
                    #(#alloc_slots_body)*
                }
            }
        }

        // Inherent methods: .read() reconstructs the value from leaf ports,
        // .write() decomposes a value into leaf ports. Mirrors the Port<T>
        // API so composite port groups feel the same as single ports.
        impl #ports_name {
            /// Reconstructs the value from arena leaf ports.
            #[allow(dead_code)]
            pub fn read(&self, arena: &::shame_gui::graph::StateArena) -> #name {
                <#name as ::shame_gui::graph::DagStruct>::read_ports(arena, self)
            }
            /// Decomposes a value into arena leaf ports.
            #[allow(dead_code)]
            pub fn write(&self, arena: &mut ::shame_gui::graph::StateArena, value: #name) {
                <#name as ::shame_gui::graph::DagStruct>::write_ports(&value, arena, self);
            }
        }

        impl ::shame_gui::graph::PortValue for #name {}

        impl ::shame_gui::graph::DagStruct for #name {
            type Ports = #ports_name;
            fn write_ports(
                &self,
                arena: &mut ::shame_gui::graph::StateArena,
                ports: &Self::Ports,
            ) {
                #(#write_ports_body)*
            }
            fn read_ports(
                arena: &::shame_gui::graph::StateArena,
                ports: &Self::Ports,
            ) -> Self {
                Self {
                    #(#read_ports_body)*
                }
            }
        }
    })
}
