"""Register the user's classified PNG/PSD parts to the unmodified SU7 UV0 atlas.

This is a source-mask reader, not an unwrap/physical-scale solver. PNGs retain the
original CSV crop coordinates. PSD layers were rearranged in the user's document
and therefore have an explicit, verified per-layer registration. The original
files are never modified. Import load_classified_masks() from an asset builder.
"""
from __future__ import annotations

import argparse
import csv
import hashlib
import json
from pathlib import Path

import numpy as np
from PIL import Image
from psd_tools import PSDImage


DEFAULT_CLASSIFIED = Path('L:/SU7/3D/车身AO_分件/按模型部件/已分类')
DEFAULT_CSV = Path('L:/SU7/3D/车身AO_分件/原图坐标清单.csv')
DEFAULT_AO = Path('L:/SU7/3D/decoded/textures/carPaint_Shape1.png')

# A classified PNG can be a hand-trimmed subset of its original model-part crop.
PNG_PARTS = {
    '前C环.png': ('su7.front-c-ring', 'carTrimCRing_Front'),
    '前保险杠.png': ('su7.front-bumper', 'carPaint_external_10'),
    '右前门（含门把手）.png': ('su7.front-right-door', 'carPaint_external_6'),
    '右后门（含门把手）.png': ('su7.rear-right-door', 'carPaint_external_7'),
    '后C环.png': ('su7.rear-c-ring', 'carTrimCRing_Back'),
    '尾翼.png': ('su7.rear-spoiler', 'carPaint_external_2'),
    '左前门（含门把手）.png': ('su7.front-left-door', 'carPaint_external_4'),
    '左后门（含门把手）.png': ('su7.rear-left-door', 'carPaint_external_5'),
    '机盖.png': ('su7.hood', 'carPaint_external_3_1'),
    '左后视镜上壳.png': ('su7.mirror-left-upper', 'carMirror_top_2'),
    '左后视镜下壳.png': ('su7.mirror-left-lower', 'carMirror_bottom_2'),
    '右后视镜上壳.png': ('su7.mirror-right-upper', 'carMirror_top_1'),
    '右后视镜下壳.png': ('su7.mirror-right-lower', 'carMirror_bottom_1'),
}

# Operations occur before resizing. Origins are UV0 atlas pixel indices.
PSD_PARTS = {
    '左后翼子板整体件': ('su7.rear-left-fender', None, None, (12, 231)),
    '右后翼子板整体件': ('su7.rear-right-fender', None, None, (12, 231)),
    '后保险杠': ('su7.rear-bumper', None, None, (12, 231)),
    '左前下翼子板': ('su7.front-left-lower-fender', 'TRANSPOSE', (100, 212), (74, 1306)),
    '右前下翼子板': ('su7.front-right-lower-fender', 'FLIP_TOP_BOTTOM', (248, 117), (208, 1826)),
    '左前上翼子板': ('su7.front-left-fender', 'ROTATE_270', (521, 149), (838, 1772)),
    '右前上翼子板': ('su7.front-right-fender', 'ROTATE_90', (520, 149), (828, 1633)),
    '油箱盖': ('su7.charge-cover', 'FLIP_TOP_BOTTOM', (70, 91), (1491, 1492)),
}


def sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def _paste_rgba(image: Image.Image, origin: tuple[int, int], size: tuple[int, int]) -> Image.Image:
    result = Image.new('RGBA', size)
    result.paste(image, origin)
    return result


def _registration_metrics(image: Image.Image, origin: tuple[int, int], ao: np.ndarray) -> dict:
    values = np.asarray(image.convert('RGBA'))
    x, y = origin
    h, w = values.shape[:2]
    assert x >= 0 and y >= 0 and x + w <= ao.shape[1] and y + h <= ao.shape[0], (origin, image.size)
    support = values[:, :, 3] > 0
    actual = values[:, :, :3].astype(np.int16)
    expected = ao[y:y+h, x:x+w, :3].astype(np.int16)
    error = np.abs(actual - expected)[support]
    if not len(error):
        raise ValueError('Classified layer has no alpha support')
    # Contrast-bearing pixels avoid giving untextured white padding most weight.
    detail = support & (values[:, :, :3].min(axis=2) < 245)
    detail_error = np.abs(actual - expected)[detail]
    return {
        'alphaPixels': int(support.sum()),
        'rgbExactPixelFraction': float(np.all(error == 0, axis=1).mean()),
        'rgbMeanAbsoluteError': float(error.mean()),
        'rgbP95AbsoluteError': float(np.percentile(error, 95)),
        'detailPixels': int(detail.sum()),
        'detailRgbMeanAbsoluteError': float(detail_error.mean()) if len(detail_error) else 0.,
    }


def _local_match(image: Image.Image, origin: tuple[int, int], ao: np.ndarray, radius: int = 12) -> dict:
    """Verify the calibration against RGB in a local translation search.

    This is deliberately not a fresh whole-atlas feature matcher: white AO areas
    and mirrored/repeated source islands make unconstrained matching ambiguous.
    """
    result = _registration_metrics(image, origin, ao)
    values = np.asarray(image.convert('RGBA'))
    detail = (values[:, :, 3] > 0) & (values[:, :, :3].min(axis=2) < 245)
    yy, xx = np.nonzero(detail)
    if len(xx) > 6000:
        chosen = np.linspace(0, len(xx)-1, 6000, dtype=np.int64)
        xx, yy = xx[chosen], yy[chosen]
    rgb = values[yy, xx, :3].astype(np.int16)
    candidates = []
    for dy in range(-radius, radius + 1):
        for dx in range(-radius, radius + 1):
            location = (origin[0] + dx, origin[1] + dy)
            if min(location) < 0 or location[0]+image.width > ao.shape[1] or location[1]+image.height > ao.shape[0]:
                continue
            expected = ao[location[1]+yy, location[0]+xx, :3].astype(np.int16)
            score = float(np.abs(rgb - expected).mean())
            candidates.append((score, dx*dx+dy*dy, location))
    best = min(candidates)
    result.update(_registration_metrics(image, best[2], ao))
    result['localSearchRadiusPx'] = radius
    result['localBestOriginPx'] = list(best[2])
    result['localBestDetailRgbMeanAbsoluteError'] = best[0]
    result['calibratedOriginLocallyOptimal'] = best[2] == origin
    return result


def _canvas_matrix(layer, operation: str | None, output_size: tuple[int, int] | None,
                   atlas_origin: tuple[int, int]) -> list[list[float]]:
    """Pixel-index affine equivalent of crop -> transpose -> resize -> paste."""
    w, h = layer.size
    if operation == 'TRANSPOSE':
        linear = np.array([[0., 1.], [1., 0.]])
        shift = np.zeros(2)
        rotated_size = (h, w)
    elif operation == 'ROTATE_270':
        linear = np.array([[0., -1.], [1., 0.]])
        shift = np.array([h-1., 0.])
        rotated_size = (h, w)
    elif operation == 'ROTATE_90':
        linear = np.array([[0., 1.], [-1., 0.]])
        shift = np.array([0., w-1.])
        rotated_size = (h, w)
    elif operation == 'FLIP_TOP_BOTTOM':
        linear = np.array([[1., 0.], [0., -1.]])
        shift = np.array([0., h-1.])
        rotated_size = (w, h)
    else:
        linear = np.eye(2)
        shift = np.zeros(2)
        rotated_size = (w, h)
    scaling = np.diag(np.asarray(output_size or rotated_size) / rotated_size)
    # Unchanged layers retain their document position before the common offset.
    origin = np.asarray(atlas_origin, dtype=float)
    if operation is None:
        origin += (layer.left, layer.top)
    full_linear = scaling @ linear
    translation = origin + scaling @ shift - full_linear @ np.asarray((layer.left, layer.top))
    return np.column_stack((full_linear, translation)).tolist()


def load_classified_masks(classified_dir: Path | str = DEFAULT_CLASSIFIED,
                          coordinate_csv: Path | str = DEFAULT_CSV,
                          ao_path: Path | str = DEFAULT_AO,
                          output_dir: Path | str | None = None,
                          include_mirrors: bool = False) -> tuple[dict[str, np.ndarray], dict]:
    """Return partId -> bool[atlasHeight,atlasWidth] and registration evidence.

    Alpha > 0 is preserved. No erosion/dilation or shape simplification is done.
    Mirror masks can be requested for audit but are excluded from body paint by
    default. The explicit rear-bumper sensor layer is subtracted from its part.
    """
    classified_dir, coordinate_csv, ao_path = map(Path, (classified_dir, coordinate_csv, ao_path))
    ao_image = Image.open(ao_path).convert('RGBA')
    ao = np.asarray(ao_image)
    with coordinate_csv.open(encoding='utf-8-sig', newline='') as source:
        rows = {r['模型部件名称']: r for r in csv.DictReader(source) if r['类型'] == '模型部件'}
    masks: dict[str, np.ndarray] = {}
    evidence = {
        'coordinateConvention': 'top-left origin; atlas x=u*width, y=v*height; original GLB TEXCOORD_0',
        'atlasPx': list(ao_image.size), 'aoFile': str(ao_path), 'aoSha256': sha256(ao_path),
        'coordinateCsv': str(coordinate_csv), 'coordinateCsvSha256': sha256(coordinate_csv),
        'alphaThreshold': '>0', 'morphology': 'none', 'sources': [], 'unassignedLayers': [],
    }
    for filename, (part, source_name) in PNG_PARTS.items():
        path = classified_dir / filename
        row = rows[source_name]
        origin = (int(row['原图左上角X']), int(row['原图左上角Y']))
        image = Image.open(path).convert('RGBA')
        assert image.size == (int(row['宽度']), int(row['高度'])), (filename, image.size, row)
        metrics = _registration_metrics(image, origin, ao)
        # The user's alpha is authoritative; crop RGB must retain atlas pixels.
        if metrics['rgbExactPixelFraction'] < .999:
            raise ValueError(f'{filename}: PNG does not match its original atlas crop: {metrics}')
        mirror = part.startswith('su7.mirror-')
        if not mirror or include_mirrors:
            masks[part] = np.asarray(_paste_rgba(image, origin, ao_image.size))[:, :, 3] > 0
        evidence['sources'].append({
            'kind': 'png', 'file': str(path), 'sha256': sha256(path), 'partId': part,
            'sourceModelPart': source_name, 'originPx': list(origin), 'sourcePx': list(image.size),
            'matrix': [[1., 0., origin[0]], [0., 1., origin[1]]],
            'includedAsBodyPaint': not mirror, **metrics,
        })
    psd_path = classified_dir / '车身分件.psd'
    psd = PSDImage.open(psd_path)
    evidence['psd'] = {'file': str(psd_path), 'sha256': sha256(psd_path), 'canvasPx': list(psd.size)}
    sensor_mask = None
    for layer in psd:
        image = layer.topil().convert('RGBA')
        if layer.name == '后保险杠传感器':
            origin = (layer.left + 12, layer.top + 231)
            sensor_mask = np.asarray(_paste_rgba(image, origin, ao_image.size))[:, :, 3] > 0
            evidence['sensorExclusion'] = {'layer': layer.name, 'originPx': list(origin),
                                           'alphaPixels': int(sensor_mask.sum())}
            continue
        if layer.name not in PSD_PARTS:
            evidence['unassignedLayers'].append({'name': layer.name, 'bounds': list(layer.bbox)})
            continue
        part, operation, resized, offset = PSD_PARTS[layer.name]
        original_size = image.size
        if operation:
            image = image.transpose(getattr(Image.Transpose, operation))
        if resized:
            image = image.resize(resized, Image.Resampling.NEAREST)
        origin = offset if operation else (layer.left + offset[0], layer.top + offset[1])
        metrics = _local_match(image, origin, ao)
        masks[part] = np.asarray(_paste_rgba(image, origin, ao_image.size))[:, :, 3] > 0
        evidence['sources'].append({
            'kind': 'psd-layer', 'layer': layer.name, 'partId': part,
            'layerBoundsPx': list(layer.bbox), 'sourcePx': list(original_size),
            'operation': operation or 'identity', 'resampling': 'nearest',
            'transformedPx': list(image.size), 'originPx': list(origin),
            'matrixFromPsdCanvasPixelIndices': _canvas_matrix(layer, operation, resized, offset),
            'calibratedOriginPx': list(origin),
            'rgbMatchedOriginIsDiagnosticOnly': True,
            **metrics,
        })
    if sensor_mask is not None:
        before = int(masks['su7.rear-bumper'].sum())
        masks['su7.rear-bumper'] &= ~sensor_mask
        evidence['sensorExclusion']['removedFromBumperPixels'] = before - int(masks['su7.rear-bumper'].sum())
    # The cover is its own part; a classification overlap cannot make both paintable.
    cover = masks['su7.charge-cover']
    overlap = int((masks['su7.rear-left-fender'] & cover).sum())
    masks['su7.rear-left-fender'] &= ~cover
    evidence['chargeCoverExcludedFromRearLeftFenderPixels'] = overlap
    evidence['partMaskPixels'] = {part: int(mask.sum()) for part, mask in masks.items()}
    evidence['notes'] = [
        'Registration constrains paint geometry in source UV0; it does not certify physical dimensions or a low-distortion unwrap.',
        'PSD front fender and charge-cover layers were resized/reoriented by the user; nearest resampling reproduces their calibrated atlas extent.',
        'Unassigned PSD layers are recorded and never silently broadcast to all parts.',
    ]
    if output_dir is not None:
        output_dir = Path(output_dir)
        output_dir.mkdir(parents=True, exist_ok=True)
        for part, mask in masks.items():
            Image.fromarray(mask.astype(np.uint8)*255).save(output_dir / f'{part}.png')
        (output_dir / 'registration.json').write_text(json.dumps(evidence, ensure_ascii=False, indent=2), encoding='utf-8')
    return masks, evidence


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--classified', type=Path, default=DEFAULT_CLASSIFIED)
    parser.add_argument('--csv', type=Path, default=DEFAULT_CSV)
    parser.add_argument('--ao', type=Path, default=DEFAULT_AO)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--include-mirrors', action='store_true')
    args = parser.parse_args()
    masks, report = load_classified_masks(args.classified, args.csv, args.ao, args.output, args.include_mirrors)
    print(json.dumps({'parts': len(masks), 'report': str(args.output/'registration.json'),
                      'psdLayers': [{k: s[k] for k in ('layer', 'originPx', 'rgbExactPixelFraction',
                                                        'detailRgbMeanAbsoluteError', 'localBestOriginPx')}
                                    for s in report['sources'] if s['kind'] == 'psd-layer']},
                     ensure_ascii=True, indent=2))
