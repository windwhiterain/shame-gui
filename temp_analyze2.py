from PIL import Image
import numpy as np
from collections import Counter

actual = np.array(Image.open(r'C:\resource\shame-gui\crates\shame-gui\tests\snapshots\form\actual.png'))
expected = np.array(Image.open(r'C:\resource\shame-gui\crates\shame-gui\tests\snapshots\form\expected.png'))
diff = np.array(Image.open(r'C:\resource\shame-gui\crates\shame-gui\tests\snapshots\form\diff.png'))

# Check if actual and expected are pixel-identical
identical = (actual == expected).all()
print(f'actual == expected pixel-for-pixel: {identical}')
if not identical:
    diff_mask = (actual != expected).any(axis=2)
    print(f'Different pixels: {diff_mask.sum()} / {diff_mask.size} ({100*diff_mask.sum()/diff_mask.size:.2f}%)')
    # Show first 10 differences
    diff_coords = np.where(diff_mask)
    for i in range(min(10, len(diff_coords[0]))):
        y, x = diff_coords[0][i], diff_coords[1][i]
        print(f'  [{y},{x}] actual={tuple(actual[y,x,:3])} expected={tuple(expected[y,x,:3])}')

print()
print('=== diff.png first 10 pixels ===')
for y in range(5):
    for x in range(10):
        r, g, b, a = diff[y, x]
        print(f'diff[{y},{x}] = RGBA({r:3d},{g:3d},{b:3d},{a:3d})')

print()
print('=== diff.png row 0 first 100 pixels ===')
row0 = diff[0, :100, :]
for i in range(0, 100, 10):
    r, g, b, a = diff[0, i]
    print(f'  x={i:3d}: RGBA({r:3d},{g:3d},{b:3d},{a:3d})')

print()
print('=== diff.png sampled at 10 positions ===')
for y in range(0, 800, 100):
    for x in range(0, 1200, 200):
        r, g, b, a = diff[y, x]
        print(f'  [{y:3d},{x:4d}]: RGBA({r:3d},{g:3d},{b:3d},{a:3d})')

# Check diff unique colors
print()
print('=== diff.png unique color stats ===')
diff_rgb = diff[:,:,:3].reshape(-1, 3)
unique = np.unique(diff_rgb, axis=0)
print(f'Unique RGB colors: {len(unique)}')
print('First 20:')
for i in range(min(20, len(unique))):
    print(f'  RGB({unique[i,0]:3d},{unique[i,1]:3d},{unique[i,2]:3d})')

# Check diff color distribution
print()
print('=== diff color distribution (every 10px) ===')
colors_d = Counter()
for y in range(0, 800, 10):
    for x in range(0, 1200, 10):
        px = tuple(diff[y, x, :3])
        colors_d[px] += 1
for (r,g,b), cnt in colors_d.most_common(15):
    print(f'  RGB({r:3d},{g:3d},{b:3d}) -> {cnt}')

# Is diff just a red version of actual?
print()
print('=== Are diff non-red pixels related to actual content? ===')
for y in range(0, 800, 50):
    for x in range(0, 1200, 100):
        dr, dg, db = diff[y, x, :3]
        ar, ag, ab = actual[y, x, :3]
        if abs(int(dr) - int(dg)) > 5 or abs(int(dg) - int(db)) > 5:
            print(f'  [{y:3d},{x:4d}] diff=({dr:3d},{dg:3d},{db:3d}) actual=({ar:3d},{ag:3d},{ab:3d})')
