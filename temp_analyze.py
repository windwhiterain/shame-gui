from PIL import Image
import numpy as np
from collections import Counter

actual = np.array(Image.open(r'C:\resource\shame-gui\crates\shame-gui\tests\snapshots\form\actual.png'))
expected = np.array(Image.open(r'C:\resource\shame-gui\crates\shame-gui\tests\snapshots\form\expected.png'))

print('=== expected.png ===')
print(f'Shape: {expected.shape}')
print(f'Min: {expected.min()}, Max: {expected.max()}, Mean: {expected.mean():.2f}')
non_dark = (expected[:,:,0] > 20) | (expected[:,:,1] > 20) | (expected[:,:,2] > 20)
print(f'Non-dark pixels (>20): {non_dark.sum()} / {non_dark.size} ({100*non_dark.sum()/non_dark.size:.2f}%)')

print()
print('Expected row activity:')
for y_start in range(0, expected.shape[0], 100):
    y_end = min(y_start + 100, expected.shape[0])
    band = expected[y_start:y_end, :, 0]
    print(f'  Rows {y_start:4d}-{y_end:4d}: mean={band.mean():5.1f} std={band.std():5.1f}')

print()
print('Expected column activity:')
for x_start in range(0, expected.shape[1], 200):
    x_end = min(x_start + 200, expected.shape[1])
    band = expected[:, x_start:x_end, 0]
    print(f'  Cols {x_start:4d}-{x_end:4d}: mean={band.mean():5.1f} std={band.std():5.1f}')

# Detailed top region of actual
print()
print('=== actual.png DETAILED TOP 200 ROWS ===')
for y_start in range(0, 200, 20):
    y_end = min(y_start + 20, 200)
    band = actual[y_start:y_end, :, :]
    print(f'Rows {y_start:3d}-{y_end:3d}: mean_R={band[:,:,0].mean():5.1f} mean_G={band[:,:,1].mean():5.1f} mean_B={band[:,:,2].mean():5.1f} std_R={band[:,:,0].std():5.1f}')

# First few rows pixel values
print()
print('First 5 rows, first 10 pixels (R,G,B):')
for y in range(5):
    parts = []
    for x in range(10):
        r, g, b = actual[y, x, :3]
        parts.append(f'({r:3d},{g:3d},{b:3d})')
    print(f'  y={y}: {" ".join(parts)}')

# Check row 40-80 which might have text
print()
print('Rows 40-80 pixel samples (every 100px x):')
for y in range(40, 81, 10):
    parts = []
    for x in range(0, 1200, 100):
        r, g, b = actual[y, x, :3]
        parts.append(f'({r:3d},{g:3d},{b:3d})')
    print(f'  y={y}: {" ".join(parts)}')

# In actual, where are the non-gray pixels?
print()
print('=== Non-gray pixels in actual (R!=G or G!=B by >2) ===')
non_gray_count = 0
for y in range(0, actual.shape[0], 2):
    for x in range(0, actual.shape[1], 2):
        r, g, b = actual[y, x, :3]
        if abs(int(r)-int(g)) > 2 or abs(int(g)-int(b)) > 2 or abs(int(r)-int(b)) > 2:
            non_gray_count += 1
print(f'Non-gray pixels (every 2px sample): {non_gray_count}')

# Look for pixels that are brighter than background
bright = (actual[:,:,0] > 100) | (actual[:,:,1] > 100) | (actual[:,:,2] > 100)
print(f'Bright pixels (>100 in any channel): {bright.sum()}')
bright_rows = np.where(bright.any(axis=1))[0]
if len(bright_rows) > 0:
    print(f'Bright pixels row range: {bright_rows.min()} to {bright_rows.max()}')
    row_counter = Counter()
    for y in range(actual.shape[0]):
        if bright[y].sum() > 0:
            row_counter[y] = bright[y].sum()
    top_rows = sorted(row_counter.items(), key=lambda x: -x[1])[:20]
    print(f'Top 20 rows with most bright pixels: {top_rows}')

# Expected: where are bright pixels?
print()
print('=== Expected: bright pixel distribution ===')
bright_e = (expected[:,:,0] > 100) | (expected[:,:,1] > 100) | (expected[:,:,2] > 100)
print(f'Bright pixels (>100): {bright_e.sum()}')
bright_e_rows = np.where(bright_e.any(axis=1))[0]
if len(bright_e_rows) > 0:
    print(f'Bright pixels row range: {bright_e_rows.min()} to {bright_e_rows.max()}')
    row_counter_e = Counter()
    for y in range(expected.shape[0]):
        if bright_e[y].sum() > 0:
            row_counter_e[y] = bright_e[y].sum()
    top_rows_e = sorted(row_counter_e.items(), key=lambda x: -x[1])[:20]
    print(f'Top 20 rows with most bright pixels: {top_rows_e}')

# Check expected colors
print()
print('=== Expected top colors (every 10px sample) ===')
colors_e = Counter()
for y in range(0, expected.shape[0], 10):
    for x in range(0, expected.shape[1], 10):
        px = tuple(expected[y, x, :3])
        colors_e[px] += 1
for (r,g,b), cnt in colors_e.most_common(20):
    print(f'  RGB({r:3d},{g:3d},{b:3d}) -> {cnt}')

# Key check: is actual all just the clear color?
print()
print('=== Actual mid-section (y=400) pixel samples ===')
for x in range(0, 1200, 100):
    r, g, b = actual[400, x, :3]
    print(f'  x={x:4d}: RGB({r:3d},{g:3d},{b:3d})')

print()
print('=== Expected mid-section (y=400) pixel samples ===')
for x in range(0, 1200, 100):
    r, g, b = expected[400, x, :3]
    print(f'  x={x:4d}: RGB({r:3d},{g:3d},{b:3d})')
