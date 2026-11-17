"""Read-only audit of classified alpha islands and their source GLB geometry.

The audit never removes pixels, edits a mapping, or assigns a semantic part.
It records disconnected source-UV islands, their mapped world-space support,
and whether unnamed PSD layers overlap the currently unclassified parts.
"""
from __future__ import annotations

import argparse
from collections import defaultdict
import importlib.util
import json
from pathlib import Path
import sys

import cv2
import numpy as np
from PIL import Image, ImageDraw
from psd_tools import PSDImage

from classified_paint_masks import (DEFAULT_AO, DEFAULT_CLASSIFIED,
                                    _registration_metrics, load_classified_masks)


NEXT = Path(__file__).resolve().parents[1]
WORKSPACE = NEXT.parents[1]


def glb_reader():
    path = NEXT / 'tools/audit-classified-uv.py'
    spec = importlib.util.spec_from_file_location('islands_glb_reader', path)
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module.Glb


def source_uv(triangles, glb, size):
    result = []
    for tri in triangles:
        source = glb.geometry(tri['id']['sourceMesh'])
        face = int(tri['id']['sourceTriangle'])
        result.append(np.asarray(tri['sourceBarycentric']) @ source['uv'][face] * size)
    return np.asarray(result)


def area3(triangles):
    return np.linalg.norm(np.cross(triangles[:, 1] - triangles[:, 0],
                                   triangles[:, 2] - triangles[:, 0]), axis=1) * .5


def raster(uv, size):
    canvas = Image.new('1', tuple(size))
    draw = ImageDraw.Draw(canvas)
    for tri in uv:
        draw.polygon([tuple(np.rint(p).astype(int)) for p in tri], fill=1)
    return np.asarray(canvas, dtype=bool)


def bbox2(points):
    return [points.min(axis=0).tolist(), points.max(axis=0).tolist()]


def label_centroids(labels, uv):
    points = uv.mean(axis=1)
    pixels = np.rint(points).astype(np.int64)
    result = np.zeros(len(points), dtype=np.int32)
    h, w = labels.shape
    valid = ((pixels[:, 0] >= 0) & (pixels[:, 0] < w) &
             (pixels[:, 1] >= 0) & (pixels[:, 1] < h))
    result[valid] = labels[pixels[valid, 1], pixels[valid, 0]]
    # A pixel-center contour and half-pixel contour can disagree at the edge.
    # Preserve that evidence rather than assigning the nearest component.
    return result


def draw_component_card(part, labels, components, out):
    color = np.zeros((*labels.shape, 3), dtype=np.uint8)
    color[:] = (24, 27, 32)
    palette = [(70, 155, 235), (236, 137, 62), (180, 110, 218),
               (98, 194, 139), (230, 99, 128), (235, 204, 79)]
    for index, component in enumerate(components):
        color[labels == component['label']] = palette[index % len(palette)]
    image = Image.fromarray(color)
    draw = ImageDraw.Draw(image)
    for index, component in enumerate(components[:20]):
        x, y, w, h = component['atlasBoundsPx']
        draw.rectangle((x, y, x+w-1, y+h-1), outline=(235, 235, 235), width=1)
        draw.text((x, y), str(index+1), fill=(255, 255, 255), stroke_width=1,
                  stroke_fill=(0, 0, 0))
    image.thumbnail((768, 768), Image.Resampling.NEAREST)
    card = Image.new('RGB', (768, 808), (16, 18, 22))
    card.paste(image, (0, 40))
    ImageDraw.Draw(card).text((12, 12), part, fill=(240, 240, 240))
    card.save(out / f'{part}.png')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--map', type=Path, default=NEXT / 'assets/paint/su7-v18b-classified-sourcefaces/paint-map.json')
    parser.add_argument('--model', type=Path, default=WORKSPACE / '3D/Blender/software/body.glb')
    parser.add_argument('--output', type=Path, default=NEXT / '.reports/classified-islands-20261111')
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=False)
    masks, registration = load_classified_masks(include_mirrors=True)
    document = json.loads(args.map.read_text(encoding='utf-8'))
    glb = glb_reader()(args.model)
    size = np.asarray(registration['atlasPx'])
    groups = defaultdict(list)
    for tri in document['triangles']:
        groups[tri['partId']].append(tri)
    parts = []
    for part, mask in masks.items():
        count, labels, stats, centroids = cv2.connectedComponentsWithStats(mask.astype(np.uint8), 8)
        triangles = groups.get(part, [])
        if triangles:
            uv = source_uv(triangles, glb, size)
            worlds = np.asarray([t['world'] for t in triangles])
            areas = area3(worlds)
            uv_twice_area = np.abs(
                (uv[:, 1, 0] - uv[:, 0, 0]) * (uv[:, 2, 1] - uv[:, 0, 1]) -
                (uv[:, 1, 1] - uv[:, 0, 1]) * (uv[:, 2, 0] - uv[:, 0, 0]))
            atlas_uv_area = float(uv_twice_area.sum() * .5)
            part_scale_mm_per_px = float(np.sqrt(areas.sum() * 1e6 / max(atlas_uv_area, 1e-30)))
            fragment_labels = label_centroids(labels, uv)
        else:
            uv, worlds, areas, fragment_labels = (np.zeros((0, 3, 2)), np.zeros((0, 3, 3)),
                                                 np.zeros(0), np.zeros(0, dtype=int))
            part_scale_mm_per_px = None
        components = []
        order = sorted(range(1, count), key=lambda i: -int(stats[i, cv2.CC_STAT_AREA]))
        for label in order:
            x, y, width, height, pixels = map(int, stats[label])
            selected = fragment_labels == label
            component = {
                'label': label, 'alphaPixels': pixels, 'alphaFraction': pixels / int(mask.sum()),
                'atlasBoundsPx': [x, y, width, height],
                'atlasCentroidPx': centroids[label].tolist(),
                'mappedFragmentCountByCentroid': int(selected.sum()),
                'mappedWorldAreaM2ByCentroid': float(areas[selected].sum()),
                'sourceFaceCountByCentroid': len({(t['id']['sourceMesh'], t['id']['sourceTriangle'])
                                                 for t, yes in zip(triangles, selected) if yes}),
            }
            if selected.any():
                world = worlds[selected].reshape(-1, 3)
                component_uv = uv[selected].reshape(-1, 2)
                component['worldBoundsM'] = bbox2(world)
                component['worldExtentsMm'] = ((world.max(axis=0) - world.min(axis=0))*1000).tolist()
                component['mappedSourceUvBoundsPx'] = bbox2(component_uv)
                component['mappedSourceUvExtentsPx'] = (component_uv.max(axis=0)-component_uv.min(axis=0)).tolist()
                component['approximateChartExtentsMmAtUniformPartScale'] = (
                    (component_uv.max(axis=0)-component_uv.min(axis=0))*part_scale_mm_per_px).tolist()
                component['sourceTriangleExamples'] = [t['id'] for t, yes in zip(triangles, selected) if yes][:4]
            components.append(component)
        mask_y, mask_x = np.nonzero(mask)
        summary = {
            'partId': part, 'classificationSource': next((item.get('file', item.get('layer'))
                for item in registration['sources'] if item.get('partId') == part), None),
            'alphaPixels': int(mask.sum()), 'alphaComponentCount8Connected': count - 1,
            'componentsAtMost4Pixels': sum(item['alphaPixels'] <= 4 for item in components),
            'componentPixelsAtMost4Pixels': sum(item['alphaPixels'] for item in components if item['alphaPixels'] <= 4),
            'atlasOverallBoundsPx': [int(mask_x.min()), int(mask_y.min()), int(mask_x.max()-mask_x.min()+1), int(mask_y.max()-mask_y.min()+1)],
            'sourceMeshes': sorted({t['id']['sourceMesh'] for t in triangles}),
            'mappedWorldAreaM2': float(areas.sum()),
            'fragmentsWhoseCentroidFallsOutsideAlpha': int((fragment_labels == 0).sum()),
            'components': components,
        }
        if triangles:
            mapped = np.asarray([t['paintUv'] for t in triangles]).reshape(-1, 2)
            summary['mappedPaintUvBoundsMm'] = bbox2(mapped)
            summary['mappedPaintUvExtentsMm'] = (mapped.max(axis=0)-mapped.min(axis=0)).tolist()
            summary['equalAreaScaleMmPerSourceUvPixel'] = part_scale_mm_per_px
            primary = components[0]
            primary_uv = uv[fragment_labels == primary['label']]
            if len(primary_uv):
                primary_points = primary_uv.reshape(-1, 2)
                summary['primaryIslandSourceUvExtentsPx'] = (primary_points.max(axis=0)-primary_points.min(axis=0)).tolist()
                summary['primaryIslandExtentsAtPartScaleMm'] = ((primary_points.max(axis=0)-primary_points.min(axis=0))*part_scale_mm_per_px).tolist()
        draw_component_card(part, labels, components, args.output)
        parts.append(summary)

    ao = np.asarray(Image.open(DEFAULT_AO).convert('RGBA'))
    unnamed = {}
    for layer in PSDImage.open(DEFAULT_CLASSIFIED / '车身分件.psd'):
        if layer.name not in {'图层 2', '不知道是啥'}:
            continue
        image = layer.topil().convert('RGBA')
        origin = (layer.left + 12, layer.top + 231)
        canvas = Image.new('RGBA', tuple(size))
        canvas.paste(image, origin)
        values = np.asarray(canvas)[:, :, 3] > 0
        unnamed[layer.name] = {'mask': values, 'report': {
            'boundsInPsdPx': list(layer.bbox), 'atlasOriginPx': list(origin),
            'registrationWithCommonRearLayerOffset': _registration_metrics(image, origin, ao),
            'note': 'RGB support only; this does not assign a semantic body part.',
        }}
    unclassified = []
    classified_union = np.logical_or.reduce(list(masks.values()))
    for part in sorted(set(groups) - set(masks)):
        triangles = groups[part]
        uv = source_uv(triangles, glb, size)
        pixels = raster(uv, size)
        world = np.asarray([t['world'] for t in triangles])
        unclassified.append({
            'partId': part, 'hasExplicitClassifiedPngOrPsdLayer': False,
            'sourceMeshes': sorted({t['id']['sourceMesh'] for t in triangles}),
            'sourceNodeNames': sorted({glb.geometry(t['id']['sourceMesh'])['nodeName'] for t in triangles}),
            'mappedFragments': len(triangles), 'mappedWorldAreaM2': float(area3(world).sum()),
            'worldBoundsM': bbox2(world.reshape(-1, 3)),
            'sourceUvBoundsPx': bbox2(uv.reshape(-1, 2)), 'sourceUvRasterPixels': int(pixels.sum()),
            'sourceUvPixelsOverlappingExistingClassifiedMasks': int((pixels & classified_union).sum()),
            'unnamedLayerCoverage': [{
                'layer': name, 'overlapPixels': int((pixels & item['mask']).sum()),
                'fractionOfPartUvRaster': float((pixels & item['mask']).sum()) / max(int(pixels.sum()), 1),
                'fractionOfLayerAlpha': float((pixels & item['mask']).sum()) / max(int(item['mask'].sum()), 1),
            } for name, item in unnamed.items()],
        })
    report = {
        'status': 'read-only-alpha-island-and-source-geometry-audit',
        'map': str(args.map), 'model': str(args.model),
        'method': '8-connected alpha components; mapped fragments attributed only by centroid pixel; retained source barycentrics reconstructed against original GLB UV0',
        'limitations': ['Centroid attribution is an area diagnostic, not exact component clipping.',
            'No component is declared removable merely because it is small or far from the largest UV island.',
            'World-space exterior visibility is not certified by UV alpha or these bounds.'],
        'parts': parts, 'unclassifiedParts': unclassified,
        'unnamedPsdLayers': [{ 'layer': name, **item['report']} for name, item in unnamed.items()],
    }
    (args.output / 'audit.json').write_text(json.dumps(report, ensure_ascii=False, indent=2), encoding='utf-8')
    print(json.dumps({'report': str(args.output / 'audit.json'),
                      'parts': [{k: p[k] for k in ('partId','alphaComponentCount8Connected','componentsAtMost4Pixels','mappedWorldAreaM2')} for p in parts],
                      'unclassifiedParts': unclassified}, ensure_ascii=False, indent=2))


if __name__ == '__main__':
    main()
