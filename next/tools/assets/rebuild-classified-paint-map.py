"""Build a new unsealed paint map constrained by classified SU7 AO masks.

This candidate preserves each retained fragment's world geometry, source face,
and source barycentrics. Classified parts use source TEXCOORD_0 coordinates and
one equal-area scale per part. It does not certify the PSD transforms or full
exterior completeness. The output directory must not already exist.

Example:
  python tools/assets/rebuild-classified-paint-map.py --uv-y v \
    --output assets/paint/su7-v17-classified-sourceuv0
"""
from __future__ import annotations

import argparse
import importlib.util
import json
import math
from pathlib import Path
import sys

import numpy as np
from PIL import Image, ImageDraw

from classified_paint_masks import load_classified_masks


NEXT = Path(__file__).resolve().parents[2]
WORKSPACE = NEXT.parents[1]
DEFAULT_INPUT = NEXT / 'assets/paint/su7-v16-classified-unfolding/paint-map.json'
DEFAULT_OUTPUT = NEXT / 'assets/paint/su7-v17-classified-sourceuv0'
DEFAULT_MODEL = WORKSPACE / '3D/Blender/software/body.glb'
DEFAULT_MASK_REPORT = NEXT / '.reports/classified-mask-registration-20261111/registration.json'


def load_glb_reader():
    path = NEXT / 'tools/assets/audit-classified-uv.py'
    spec = importlib.util.spec_from_file_location('audit_classified_uv_reader', path)
    if spec is None or spec.loader is None:
        raise RuntimeError(f'Cannot load GLB reader: {path}')
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module.Glb


def interior_samples(order: int = 6) -> np.ndarray:
    """Barycentric lattice with no boundary points (21 deterministic samples)."""
    points = []
    for i in range(order):
        for j in range(order - i):
            b = (i + 1 / 3) / order
            c = (j + 1 / 3) / order
            points.append((1 - b - c, b, c))
    return np.asarray(points, dtype=np.float32)


def area2(triangles: np.ndarray) -> np.ndarray:
    a = triangles[:, 1] - triangles[:, 0]
    b = triangles[:, 2] - triangles[:, 0]
    return np.abs(a[:, 0] * b[:, 1] - a[:, 1] * b[:, 0]) * .5


def area3(triangles: np.ndarray) -> np.ndarray:
    return np.linalg.norm(np.cross(triangles[:, 1] - triangles[:, 0],
                                   triangles[:, 2] - triangles[:, 0]), axis=1) * .5


def identity_key(value: dict) -> tuple:
    return tuple(value[k] for k in ('sourceMesh', 'primitive', 'sourceTriangle', 'fragment'))


def triangle_uvs(triangles: list[dict], glb, width: int, height: int,
                 y_convention: str) -> np.ndarray:
    uv_sources = []
    barycentrics = []
    for triangle in triangles:
        ident = triangle['id']
        geometry = glb.geometry(ident['sourceMesh'])
        face = int(ident['sourceTriangle'])
        if not 0 <= face < len(geometry['uv']):
            raise ValueError(f'Source triangle outside GLB: {ident}')
        uv_sources.append(geometry['uv'][face].astype(np.float32))
        barycentrics.append(np.asarray(triangle['sourceBarycentric'], dtype=np.float32))
    source_uv = np.asarray(uv_sources, dtype=np.float32)
    barycentric = np.asarray(barycentrics, dtype=np.float32)
    uv = np.einsum('tbc,tcd->tbd', barycentric, source_uv, optimize=True)
    uv[:, :, 0] *= width
    if y_convention == 'v':
        uv[:, :, 1] *= height
    else:
        uv[:, :, 1] = (1 - uv[:, :, 1]) * height
    return uv.astype(np.float64)


def sample_mask(mask: np.ndarray, uv_px: np.ndarray, samples: np.ndarray) -> np.ndarray:
    points = np.einsum('sc,tcd->tsd', samples, uv_px, optimize=True)
    pixels = np.rint(points).astype(np.int64)
    h, w = mask.shape
    valid = ((pixels[:, :, 0] >= 0) & (pixels[:, :, 0] < w) &
             (pixels[:, :, 1] >= 0) & (pixels[:, :, 1] < h))
    accepted = np.zeros(valid.shape, dtype=bool)
    accepted[valid] = mask[pixels[:, :, 1][valid], pixels[:, :, 0][valid]]
    return accepted.mean(axis=1)


def rasterize(uv: np.ndarray, size: tuple[int, int], supersample: int = 1) -> np.ndarray:
    if supersample < 1:
        raise ValueError('supersample must be positive')
    width, height = size
    image = Image.new('1', (width * supersample, height * supersample), 0)
    draw = ImageDraw.Draw(image)
    for triangle in uv:
        draw.polygon([tuple(np.rint(p * supersample).astype(int)) for p in triangle], fill=1)
    pixels = np.asarray(image, dtype=bool)
    if supersample == 1:
        return pixels
    coverage = pixels.reshape(height, supersample, width, supersample).mean(axis=(1, 3))
    return coverage >= .5


def paste_preview_layer(target: Image.Image, mask: np.ndarray, color: tuple[int, int, int]):
    color_layer = Image.new('RGB', target.size, color)
    target.paste(color_layer, (0, 0), Image.fromarray(mask.astype(np.uint8) * 255))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--input', type=Path, default=DEFAULT_INPUT)
    parser.add_argument('--model', type=Path, default=DEFAULT_MODEL)
    parser.add_argument('--output', type=Path, default=DEFAULT_OUTPUT)
    parser.add_argument('--uv-y', choices=('v', '1-v'), default='v',
                        help='Convert source V to atlas Y using v (default) or 1-v.')
    parser.add_argument('--sample-order', type=int, default=6,
                        help='Interior barycentric lattice order; 6 yields 21 samples per fragment.')
    parser.add_argument('--keep-threshold', type=float, default=.5,
                        help='Keep a classified fragment at or above this alpha-mask sample fraction.')
    parser.add_argument('--atlas-width', type=int, default=2048)
    parser.add_argument('--atlas-height', type=int, default=2048)
    parser.add_argument('--mask-report', type=Path, default=DEFAULT_MASK_REPORT)
    options = parser.parse_args()
    if options.output.exists():
        raise FileExistsError(f'Refusing to overwrite existing candidate directory: {options.output}')
    if not 0 <= options.keep_threshold <= 1:
        raise ValueError('--keep-threshold must be between 0 and 1')
    if options.sample_order < 2:
        raise ValueError('--sample-order must be >= 2')
    if options.atlas_width <= 0 or options.atlas_height <= 0:
        raise ValueError('Atlas dimensions must be positive')

    masks, registration = load_classified_masks()
    for part, mask in masks.items():
        if mask.shape != (options.atlas_height, options.atlas_width):
            raise ValueError(f'{part}: mask size {mask.shape[::-1]} does not match requested atlas')
    mask_source = json.loads(options.mask_report.read_text(encoding='utf-8')) if options.mask_report.exists() else registration
    source_map = json.loads(options.input.read_text(encoding='utf-8'))
    glb_reader = load_glb_reader()
    glb = glb_reader(options.model)
    document = json.loads(json.dumps(source_map))
    if document.get('modelFingerprint'):
        import hashlib
        digest = hashlib.sha256(options.model.read_bytes()).hexdigest()
        if digest != document['modelFingerprint']:
            raise ValueError('Source GLB fingerprint does not match the input paint map')

    grouped: dict[str, list[dict]] = {}
    for triangle in document['triangles']:
        grouped.setdefault(triangle['partId'], []).append(triangle)

    samples = interior_samples(options.sample_order)
    report = {
        'status': 'v17-classified-mask-filtered-candidate-requires-semantic-review',
        'semanticApproval': False,
        'inputMap': str(options.input.resolve()),
        'inputModel': str(options.model.resolve()),
        'coordinateConvention': {
            'uvY': options.uv_y,
            'atlasPixels': [options.atlas_width, options.atlas_height],
            'pointMapping': 'x=u*width; y=v*height' if options.uv_y == 'v' else 'x=u*width; y=(1-v)*height',
            'samplesPerFragment': len(samples),
            'sampleOrder': options.sample_order,
            'keepThreshold': options.keep_threshold,
            'alphaRule': 'mask alpha > 0',
            'scale': 'one uniform mm-per-source-UV-pixel factor per classified part, chosen to match summed world and UV triangle areas',
        },
        'maskRegistration': {
            'report': str(options.mask_report.resolve()),
            'psdRgbResiduals': [
                {'partId': item.get('partId'), 'layer': item.get('layer'),
                 'detailRgbMeanAbsoluteError': item.get('detailRgbMeanAbsoluteError'),
                 'exactPixelFraction': item.get('rgbExactPixelFraction'),
                 'calibratedOriginPx': item.get('calibratedOriginPx', item.get('originPx'))}
                for item in mask_source.get('sources', []) if item.get('kind') == 'psd-layer'
            ],
            'psdRegistrationStatus': 'rear layers RGB-verified; front fenders and charge cover retain transform candidates with nonzero RGB residuals',
        },
        'connectionCleanup': {},
        'parts': [],
        'warnings': [
            'This candidate filters the input map; it does not recover source faces absent from the input map.',
            'Triangle-centroid lattice sampling approximates the classified alpha boundary; no exact polygon clipping is performed.',
            'PSD front-fender and charge-cover registration remains uncertain and is reported, not treated as approved.',
            'Source UV0 may contain seams, disconnected islands, overlap, or distortion; use the included preview and follow-up audits before runtime adoption.',
        ],
    }

    preview_order = [
        'su7.front-bumper', 'su7.front-c-ring', 'su7.hood', 'su7.front-left-fender',
        'su7.front-right-fender', 'su7.front-left-lower-fender', 'su7.front-right-lower-fender',
        'su7.front-left-door', 'su7.front-right-door', 'su7.rear-left-door', 'su7.rear-right-door',
        'su7.rear-left-fender', 'su7.rear-right-fender', 'su7.charge-cover', 'su7.rear-spoiler',
        'su7.rear-bumper', 'su7.rear-c-ring',
    ]
    colors = [(235, 90, 66), (80, 176, 238), (255, 205, 70), (176, 111, 222),
              (74, 203, 165), (242, 146, 66), (85, 132, 225), (235, 110, 160),
              (125, 202, 95), (229, 157, 76), (112, 191, 220), (208, 101, 98),
              (105, 154, 218), (242, 202, 92), (185, 126, 226), (85, 194, 158),
              (237, 127, 196)]
    left = Image.new('RGB', (options.atlas_width, options.atlas_height), (22, 24, 28))
    right = Image.new('RGB', (options.atlas_width, options.atlas_height), (22, 24, 28))
    draw_left, draw_right = ImageDraw.Draw(left), ImageDraw.Draw(right)
    draw_left.rectangle((0, 0, options.atlas_width, 28), fill=(37, 41, 47))
    draw_right.rectangle((0, 0, options.atlas_width, 28), fill=(37, 41, 47))
    draw_left.text((10, 8), 'CLASSIFIED ALPHA MASKS', fill=(240, 240, 240))
    draw_right.text((10, 8), f'SELECTED SOURCE UV0 TRIANGLES · y={options.uv_y}', fill=(240, 240, 240))

    old_count = len(document['triangles'])
    retained_ids = set()
    for part, triangles in grouped.items():
        mask = masks.get(part)
        if mask is None:
            uv = np.asarray([t['paintUv'] for t in triangles], dtype=np.float64)
            low = uv.reshape(-1, 2).min(axis=0)
            for t in triangles:
                t['paintUv'] = (np.asarray(t['paintUv'], dtype=np.float64) - low).tolist()
                retained_ids.add(identity_key(t['id']))
            report['parts'].append({
                'partId': part, 'classificationApplied': False, 'status': 'retained-existing-part',
                'inputFragments': len(triangles), 'retainedFragments': len(triangles),
                'worldAreaM2': float(area3(np.asarray([t['world'] for t in triangles])).sum()),
                'paintUvOriginShiftMm': low.tolist(),
                'note': 'Existing part geometry and local shape retained; only its positive coordinate origin was shifted.',
            })
            continue

        uv_source = triangle_uvs(triangles, glb, options.atlas_width, options.atlas_height, options.uv_y)
        membership = sample_mask(mask, uv_source, samples)
        selected = membership >= options.keep_threshold
        selected_tris = [t for t, keep in zip(triangles, selected) if keep]
        selected_uv = uv_source[selected]
        selected_world = np.asarray([t['world'] for t in selected_tris], dtype=np.float64)
        all_world = np.asarray([t['world'] for t in triangles], dtype=np.float64)
        world_areas = area3(all_world)
        selected_world_areas = area3(selected_world) if len(selected_tris) else np.zeros(0)
        selected_uv_areas = area2(selected_uv) if len(selected_tris) else np.zeros(0)
        world_area = float(selected_world_areas.sum())
        uv_area = float(selected_uv_areas.sum())
        if len(selected_tris) == 0 or world_area <= 0 or uv_area <= 0:
            raise ValueError(f'{part}: no nondegenerate fragments survive mask threshold {options.keep_threshold}')
        scale_mm_per_px = math.sqrt(world_area * 1e6 / uv_area)
        origin = selected_uv.reshape(-1, 2).min(axis=0)
        for triangle, uv in zip(selected_tris, selected_uv):
            triangle['paintUv'] = ((uv - origin) * scale_mm_per_px).tolist()
            triangle['chartId'] = f'{part}/source-uv0'
            retained_ids.add(identity_key(triangle['id']))
        input_map_mask = rasterize(uv_source, (options.atlas_width, options.atlas_height))
        selected_mask = rasterize(selected_uv, (options.atlas_width, options.atlas_height))
        input_map_2x = rasterize(uv_source, (options.atlas_width, options.atlas_height), supersample=2)
        selected_map_2x = rasterize(selected_uv, (options.atlas_width, options.atlas_height), supersample=2)
        inter = int(np.count_nonzero(selected_mask & mask))
        map_pixels = int(selected_mask.sum())
        classified_pixels = int(mask.sum())
        input_inter = int(np.count_nonzero(input_map_mask & mask))
        threshold_removed_pixels = int(np.count_nonzero(mask & input_map_mask & ~selected_mask))
        missing_input_pixels = int(np.count_nonzero(mask & ~input_map_mask))
        missing_input_2x_pixels = int(np.count_nonzero(mask & ~input_map_2x))
        missing_selected_2x_pixels = int(np.count_nonzero(mask & ~selected_map_2x))
        rasterGapsRecoveredBy2x = int(np.count_nonzero(mask & ~selected_mask & selected_map_2x))
        part_report = {
            'partId': part,
            'classificationApplied': True,
            'fragmentsBeforeMask': len(triangles),
            'fragmentsRetained': len(selected_tris),
            'fragmentsRemoved': len(triangles) - len(selected_tris),
            'fragmentRetentionRatio': len(selected_tris) / max(len(triangles), 1),
            'maskSamplesInsideFraction': float(membership.mean()),
            'retainedFragmentsMeanMaskCoverage': float(membership[selected].mean()),
            'removedFragmentsMeanMaskCoverage': float(membership[~selected].mean()) if np.any(~selected) else None,
            'worldAreaBeforeMaskM2': float(world_areas.sum()),
            'worldAreaRetainedM2': world_area,
            'worldAreaRetainedRatio': world_area / max(float(world_areas.sum()), 1e-30),
            'sourceUvAreaRetainedPx2': uv_area,
            'equalAreaScaleMmPerUvPixel': scale_mm_per_px,
            'sourceUvOriginPx': origin.tolist(),
            'maskAlphaPixels': classified_pixels,
            'inputMapTriangleRasterPixels': int(input_map_mask.sum()),
            'inputMapRasterPrecision': input_inter / max(int(input_map_mask.sum()), 1),
            'inputMapRasterRecall': input_inter / max(classified_pixels, 1),
            'maskPixelsCoveredByInputMapBeforeThreshold': input_inter,
            'maskPixelsRemovedByKeepThreshold': threshold_removed_pixels,
            'maskPixelsMissingFromInputMapRaster': missing_input_pixels,
            'maskPixelsMissingFromInputMap2xRaster': missing_input_2x_pixels,
            'maskPixelsMissingFromSelected2xRaster': missing_selected_2x_pixels,
            'selectedRasterGapPixelsRecoveredBy2x': rasterGapsRecoveredBy2x,
            'selectedRasterGapRecoveryBy2xFraction': rasterGapsRecoveredBy2x / max(int(np.count_nonzero(mask & ~selected_mask)), 1),
            'thresholdLossFractionOfMask': threshold_removed_pixels / max(classified_pixels, 1),
            'inputMapCoverageGapFractionOfMask': missing_input_pixels / max(classified_pixels, 1),
            'selectedTriangleRasterPixels': map_pixels,
            'maskIntersectionPixels': inter,
            'selectedRasterPrecision': inter / max(map_pixels, 1),
            'selectedRasterRecall': inter / max(classified_pixels, 1),
            'selectedRasterIoU': inter / max(map_pixels + classified_pixels - inter, 1),
            'psdRgbDetailMae': next((s.get('detailRgbMeanAbsoluteError') for s in mask_source.get('sources', [])
                                     if s.get('partId') == part and s.get('kind') == 'psd-layer'), None),
            'sourceUvOverlaps': 'not calculated in builder; run audit-paint-uv.py before runtime adoption',
        }
        report['parts'].append(part_report)
        color = colors[preview_order.index(part)] if part in preview_order else (220, 220, 220)
        paste_preview_layer(left, mask, color)
        paste_preview_layer(right, selected_mask, color)

    document['triangles'] = [t for t in document['triangles'] if identity_key(t['id']) in retained_ids]
    old_connections = document.get('connections', [])
    valid_connections = []
    for connection in old_connections:
        from_id = identity_key(connection.get('from', {}).get('triangle', {}))
        to_id = identity_key(connection.get('to', {}).get('triangle', {}))
        if from_id in retained_ids and to_id in retained_ids:
            valid_connections.append(connection)
    document['connections'] = valid_connections
    report['connectionCleanup'] = {
        'before': len(old_connections), 'after': len(valid_connections),
        'removedStaleReferences': len(old_connections) - len(valid_connections),
    }
    document['paintFingerprint'] = ''
    report['classificationMasksWithoutInputPart'] = sorted(set(masks) - set(grouped))
    report['totals'] = {
        'inputFragments': old_count,
        'outputFragments': len(document['triangles']),
        'removedFragments': old_count - len(document['triangles']),
        'classifiedParts': sum(bool(p.get('classificationApplied')) for p in report['parts']),
        'retainedExistingParts': sum(not p.get('classificationApplied') for p in report['parts']),
        'connectionsBefore': len(old_connections), 'connectionsAfter': len(valid_connections),
    }

    options.output.mkdir(parents=True, exist_ok=False)
    (options.output / 'paint-map-unsealed.json').write_text(
        json.dumps(document, separators=(',', ':')), encoding='utf-8')
    (options.output / 'build-report.json').write_text(
        json.dumps(report, ensure_ascii=False, indent=2), encoding='utf-8')
    preview = Image.new('RGB', (options.atlas_width * 2, options.atlas_height + 30), (14, 16, 19))
    preview.paste(left, (0, 30))
    preview.paste(right, (options.atlas_width, 30))
    header = ImageDraw.Draw(preview)
    header.text((10, 10), 'Classified source masks', fill=(238, 238, 238))
    header.text((options.atlas_width + 10, 10), 'Selected source triangles', fill=(238, 238, 238))
    preview.save(options.output / 'mask-preview.png')
    print(json.dumps(report['totals'], ensure_ascii=True, indent=2))
    for part in report['parts']:
        if part.get('classificationApplied'):
            print('PART', part['partId'], 'kept', part['fragmentsRetained'], '/',
                  part['fragmentsBeforeMask'], 'IoU', round(part['selectedRasterIoU'], 4),
                  'area', round(part['worldAreaRetainedRatio'], 4), flush=True)


if __name__ == '__main__':
    main()
