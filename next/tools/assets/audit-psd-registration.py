"""Search local scale/translation candidates for the five transformed PSD layers.

Writes registration evidence only. It does not edit the source PSD, masks,
mapping assets, or runtime registration constants.
"""
from __future__ import annotations

import argparse
import json
from pathlib import Path

import cv2
import numpy as np
from PIL import Image
from psd_tools import PSDImage

from classified_paint_masks import DEFAULT_AO, DEFAULT_CLASSIFIED, PSD_PARTS


NEXT = Path(__file__).resolve().parents[2]
INTERPOLATIONS = {'nearest': cv2.INTER_NEAREST, 'linear': cv2.INTER_LINEAR,
                  'cubic': cv2.INTER_CUBIC}
ORIENTATIONS = {
    'identity': None,
    'flipLeftRight': Image.Transpose.FLIP_LEFT_RIGHT,
    'flipTopBottom': Image.Transpose.FLIP_TOP_BOTTOM,
    'rotate90': Image.Transpose.ROTATE_90,
    'rotate180': Image.Transpose.ROTATE_180,
    'rotate270': Image.Transpose.ROTATE_270,
    'transpose': Image.Transpose.TRANSPOSE,
    'transverse': Image.Transpose.TRANSVERSE,
}


def make_template(image, size, interpolation):
    pixels = np.asarray(image.convert('RGBA'))
    color = cv2.resize(pixels[:, :, :3].astype(np.float32), size, interpolation=interpolation)
    alpha = cv2.resize(pixels[:, :, 3], size, interpolation=cv2.INTER_NEAREST)
    support = cv2.erode((alpha >= 250).astype(np.uint8), np.ones((3, 3), np.uint8)) > 0
    detail = support & (color.min(axis=2) < 245)
    if int(detail.sum()) < 100:
        raise ValueError('insufficient opaque detail support')
    return color, detail


def best_translation(ao, color, detail, original_origin, radius):
    w, h = color.shape[1], color.shape[0]
    ox, oy = original_origin
    left, top = max(0, ox-radius), max(0, oy-radius)
    right, bottom = min(ao.shape[1], ox+radius+w), min(ao.shape[0], oy+radius+h)
    roi = ao[top:bottom, left:right]
    if roi.shape[0] < h or roi.shape[1] < w:
        return None
    # The textures are grayscale AO, but keeping RGB in the objective records
    # any differences between channels instead of silently discarding them.
    result = cv2.matchTemplate(roi, color, cv2.TM_SQDIFF, mask=detail.astype(np.uint8))
    _, _, location, _ = cv2.minMaxLoc(result)
    origin = (left + location[0], top + location[1])
    actual = ao[origin[1]:origin[1]+h, origin[0]:origin[0]+w]
    residual = np.abs(actual-color)[detail]
    return {
        'originPx': list(origin), 'sizePx': [w, h], 'detailPixels': int(detail.sum()),
        'detailRgbMeanAbsoluteError': float(residual.mean()),
        'detailRgbRootMeanSquareError': float(np.sqrt((residual*residual).mean())),
        'detailRgbP95AbsoluteError': float(np.percentile(residual, 95)),
        'detailRgbExactPixelFraction': float((residual.max(axis=1) < .5).mean()),
    }


def at_original(ao, color, detail, origin):
    x, y = origin
    h, w = color.shape[:2]
    delta = np.abs(ao[y:y+h, x:x+w] - color)[detail]
    return {'originPx': list(origin), 'sizePx': [w, h], 'detailPixels': int(detail.sum()),
            'detailRgbMeanAbsoluteError': float(delta.mean()),
            'detailRgbRootMeanSquareError': float(np.sqrt((delta*delta).mean())),
            'detailRgbP95AbsoluteError': float(np.percentile(delta, 95)),
            'detailRgbExactPixelFraction': float((delta.max(axis=1) < .5).mean())}


def search_alternate_orientations(ao, layer_image, original_origin, radius):
    candidates = []
    for orientation, method in ORIENTATIONS.items():
        oriented = layer_image if method is None else layer_image.transpose(method)
        for scale in np.arange(.70, 1.0501, .01):
            size = tuple(max(1, round(value * float(scale))) for value in oriented.size)
            for interpolation_name, interpolation in INTERPOLATIONS.items():
                color, detail = make_template(oriented, size, interpolation)
                candidate = best_translation(ao, color, detail, original_origin, radius)
                if candidate is not None:
                    candidate['orientation'] = orientation
                    candidate['interpolation'] = interpolation_name
                    candidate['uniformScaleFromOrientedPsd'] = float(scale)
                    candidates.append(candidate)
    if not candidates:
        return []
    coarse = min(candidates, key=lambda c: c['detailRgbRootMeanSquareError'])
    orientation = ORIENTATIONS[coarse['orientation']]
    oriented = layer_image if orientation is None else layer_image.transpose(orientation)
    fine = []
    best_w, best_h = coarse['sizePx']
    for width in range(max(1, best_w-4), best_w+5):
        for height in range(max(1, best_h-4), best_h+5):
            for interpolation_name, interpolation in INTERPOLATIONS.items():
                color, detail = make_template(oriented, (width, height), interpolation)
                candidate = best_translation(ao, color, detail, original_origin, radius)
                if candidate is not None:
                    candidate['orientation'] = coarse['orientation']
                    candidate['interpolation'] = interpolation_name
                    candidate['uniformScaleFromOrientedPsd'] = None
                    fine.append(candidate)
    combined = fine + candidates
    combined.sort(key=lambda c: c['detailRgbRootMeanSquareError'])
    distinct = []
    seen = set()
    for candidate in combined:
        key = (candidate.get('orientation'), tuple(candidate['sizePx']),
               tuple(candidate['originPx']), candidate['interpolation'])
        if key not in seen:
            seen.add(key)
            distinct.append(candidate)
    return distinct[:20]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, default=NEXT / '.reports/psd-registration-search-20261111')
    parser.add_argument('--radius', type=int, default=48)
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=False)
    ao_image = Image.open(DEFAULT_AO).convert('RGB')
    ao = np.asarray(ao_image).astype(np.float32)
    layers = {layer.name: layer for layer in PSDImage.open(DEFAULT_CLASSIFIED / '车身分件.psd')}
    results = []
    for name, (part, operation, current_size, current_origin) in PSD_PARTS.items():
        if operation is None:
            continue
        layer = layers[name]
        raw_image = layer.topil().convert('RGBA')
        image = raw_image.transpose(getattr(Image.Transpose, operation))
        template, detail = make_template(image, current_size, cv2.INTER_NEAREST)
        baseline = at_original(ao, template, detail, current_origin)
        trials = []
        seen = set()
        for scale in np.arange(.90, 1.1001, .005):
            size = tuple(max(1, round(value*float(scale))) for value in current_size)
            if size in seen:
                continue
            seen.add(size)
            for interpolation_name, interpolation in INTERPOLATIONS.items():
                color, detail = make_template(image, size, interpolation)
                trial = best_translation(ao, color, detail, current_origin, args.radius)
                if trial is not None:
                    trial['interpolation'] = interpolation_name
                    trials.append(trial)
        coarse_best = min(trials, key=lambda t: t['detailRgbRootMeanSquareError'])
        best_w, best_h = coarse_best['sizePx']
        for width in range(max(1, best_w-4), best_w+5):
            for height in range(max(1, best_h-4), best_h+5):
                size = (width, height)
                for interpolation_name, interpolation in INTERPOLATIONS.items():
                    color, detail = make_template(image, size, interpolation)
                    trial = best_translation(ao, color, detail, current_origin, args.radius)
                    if trial is not None:
                        trial['interpolation'] = interpolation_name
                        trials.append(trial)
        trials.sort(key=lambda trial: trial['detailRgbRootMeanSquareError'])
        best = trials[0]
        # Keep distinct dimensions/origins; repeated candidates occur in the two
        # search passes and should not masquerade as independent evidence.
        distinct = []
        distinct_keys = set()
        for trial in trials:
            key = (tuple(trial['originPx']), tuple(trial['sizePx']), trial['interpolation'])
            if key not in distinct_keys:
                distinct.append(trial)
                distinct_keys.add(key)
        color, detail = make_template(image, tuple(best['sizePx']), INTERPOLATIONS[best['interpolation']])
        x, y = best['originPx']
        crop = np.asarray(ao_image.crop((x, y, x+best['sizePx'][0], y+best['sizePx'][1])))
        candidate = np.clip(color, 0, 255).astype(np.uint8)
        error = np.clip(np.abs(crop.astype(np.float32)-color)*8, 0, 255).astype(np.uint8)
        preview = Image.new('RGB', (best['sizePx'][0]*3, best['sizePx'][1]), (24, 27, 32))
        for index, pixels in enumerate((crop, candidate, error)):
            preview.paste(Image.fromarray(pixels), (best['sizePx'][0]*index, 0))
        preview.save(args.output / f'{part}.png')
        item = {
            'partId': part, 'layer': name, 'operation': operation,
            'unrotatedPsdSizePx': list(layer.size), 'orientedPsdSizePx': list(image.size),
            'current': baseline, 'bestCandidate': best,
            'meanAbsoluteErrorReductionFraction': 1-best['detailRgbMeanAbsoluteError']/baseline['detailRgbMeanAbsoluteError'],
            'bestScaleRelativeToCurrent': [best['sizePx'][i]/current_size[i] for i in range(2)],
            'bestScaleRelativeToOrientedPsd': [best['sizePx'][i]/image.size[i] for i in range(2)],
            'topDistinctCandidates': distinct[:12],
            'approved': False,
            'warning': 'A local RGB match is registration evidence; source-triangle exterior/part semantics and a visual overlay still need independent validation.',
        }
        if best['detailRgbRootMeanSquareError'] > .5:
            item['alternateOrientationSearch'] = {
                'range': '8 dihedral orientations; isotropic source scale 0.70..1.05 then independent +/-4px refinement; local translation around calibrated origin',
                'topCandidates': search_alternate_orientations(ao, raw_image, current_origin, args.radius),
                'approved': False,
            }
        results.append(item)
        print(json.dumps({key: item[key] for key in ('partId','current','bestCandidate','meanAbsoluteErrorReductionFraction')}, ensure_ascii=True), flush=True)
    report = {'status': 'read-only-local-psd-scale-translation-search',
              'search': {'translationRadiusPx': args.radius,
                         'coarseUniformScaleRelativeToCurrent': [.90, 1.10, .005],
                         'fineIndependentDimensionRadiusPx': 4,
                         'objective': 'opaque detail RGB squared residual, excluding 1-pixel alpha boundary',
                         'interpolations': list(INTERPOLATIONS)},
              'sourceAo': str(DEFAULT_AO), 'sourcePsd': str(DEFAULT_CLASSIFIED / '车身分件.psd'),
              'results': results}
    (args.output / 'audit.json').write_text(json.dumps(report, ensure_ascii=False, indent=2), encoding='utf-8')


if __name__ == '__main__':
    main()
