"""Tidy classified mask islands and restore exact UV0 for unclassified legacy panels.

World triangles, source triangle identity, and source barycentrics are retained.
Disconnected classified islands are moved as rigid charts so small handles remain
visible beside their primary panel instead of stretching the canvas dimensions.
Unclassified trunk/sill mappings are rebuilt from exact source UV0 lineage.
"""
from __future__ import annotations

import argparse
import importlib.util
import json
import math
from collections import defaultdict
from pathlib import Path
import sys

import numpy as np
from PIL import Image, ImageDraw, ImageFont


NEXT = Path(__file__).resolve().parents[2]
DEFAULT_INPUT = NEXT / 'assets/paint/su7-v20-psd-registered-sourcefaces/paint-map-unsealed.json'
DEFAULT_OUTPUT = NEXT / 'assets/paint/su7-v21-classified-only'
DEFAULT_MODEL = NEXT.parents[1] / '3D/Blender/software/body.glb'


def load_glb(path: Path):
    source = NEXT / 'tools/assets/audit-classified-uv.py'
    spec = importlib.util.spec_from_file_location('classified_uv_reader_v19', source)
    if spec is None or spec.loader is None:
        raise RuntimeError(f'Cannot load GLB reader: {source}')
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module.Glb(path)


def area3(world: np.ndarray) -> np.ndarray:
    return np.linalg.norm(np.cross(world[:, 1] - world[:, 0],
                                   world[:, 2] - world[:, 0]), axis=1) * .5


def root(parent: list[int], item: int) -> int:
    while parent[item] != item:
        parent[item] = parent[parent[item]]
        item = parent[item]
    return item


def source_face_roots(geometry: dict) -> list[int]:
    """Find source UV charts before clipping, so clipped slivers share their original chart."""
    face_uv = geometry['uv']
    parent = list(range(len(face_uv)))
    edges: dict[tuple, int] = {}
    for index, uv in enumerate(face_uv):
        for edge in range(3):
            a = tuple(np.rint(uv[edge] * 1e7).astype(np.int64))
            b = tuple(np.rint(uv[(edge + 1) % 3] * 1e7).astype(np.int64))
            key = (a, b) if a <= b else (b, a)
            other = edges.get(key)
            if other is None:
                edges[key] = index
            else:
                parent[root(parent, index)] = root(parent, other)
    return [root(parent, index) for index in range(len(parent))]


def islands(triangles: list[dict]) -> list[list[int]]:
    """Connected UV islands from shared chart edges, including triangulation cuts."""
    parent = list(range(len(triangles)))
    edges: dict[tuple, int] = {}
    for index, triangle in enumerate(triangles):
        uv = np.asarray(triangle['paintUv'], dtype=np.float64)
        for edge in range(3):
            a = tuple(np.rint(uv[edge] / .01).astype(np.int64))
            b = tuple(np.rint(uv[(edge + 1) % 3] / .01).astype(np.int64))
            key = (a, b) if a <= b else (b, a)
            other = edges.get(key)
            if other is None:
                edges[key] = index
            else:
                parent[root(parent, index)] = root(parent, other)
    groups: dict[int, list[int]] = defaultdict(list)
    for index in range(len(triangles)):
        groups[root(parent, index)].append(index)
    return list(groups.values())


def restore_source_uv(triangles: list[dict], glb, atlas_size: tuple[int, int]) -> dict:
    geometries = {}
    source_uv, world = [], []
    source_roots, source_faces = [], []
    for triangle in triangles:
        ident = triangle['id']
        mesh_id = ident['sourceMesh']
        geometry = geometries.setdefault(mesh_id, glb.geometry(mesh_id))
        face = int(ident['sourceTriangle'])
        source_faces.append(face)
        if 'sourceFaceRoots' not in geometry:
            geometry['sourceFaceRoots'] = source_face_roots(geometry)
        source_roots.append(geometry['sourceFaceRoots'][face])
        bary = np.asarray(triangle['sourceBarycentric'], dtype=np.float64)
        source_uv.append(bary @ geometry['uv'][face].astype(np.float64) * atlas_size)
        world.append(np.asarray(triangle['world'], dtype=np.float64))
    source_uv = np.asarray(source_uv)
    world = np.asarray(world)
    edge_a = source_uv[:, 1] - source_uv[:, 0]
    edge_b = source_uv[:, 2] - source_uv[:, 0]
    uv_area = np.abs(edge_a[:, 0] * edge_b[:, 1] - edge_a[:, 1] * edge_b[:, 0]) * .5
    world_areas = area3(world)
    valid = (uv_area > 1e-9) & (world_areas > 1e-12)
    if not np.all(valid):
        raise ValueError('Degenerate source UV/world triangle in an unclassified legacy panel')
    scale = math.sqrt(float(world_areas.sum()) * 1e6 / float(uv_area.sum()))
    origin = source_uv.reshape((-1, 2)).min(axis=0)
    mapped_uv = (source_uv - origin) * scale
    chart_groups: dict[int, list[int]] = defaultdict(list)
    for index, source_root in enumerate(source_roots):
        chart_groups[source_root].append(index)
    chart_data = []
    for source_root, ids in chart_groups.items():
        points = mapped_uv[ids].reshape((-1, 2))
        chart_data.append({'root': source_root, 'ids': ids, 'low': points.min(axis=0),
                           'high': points.max(axis=0), 'area': float(world_areas[ids].sum())})
    chart_data.sort(key=lambda item: (-item['area'], tuple(item['low'])))
    primary = chart_data[0]
    primary_extent = primary['high'] - primary['low']
    cursor_x, cursor_y, shelf_height = 0., float(primary_extent[1]) + 20., 0.
    for ordinal, chart in enumerate(chart_data):
        if ordinal == 0:
            offset = -chart['low']
        else:
            extent = chart['high'] - chart['low']
            if cursor_x > 0 and cursor_x + extent[0] > max(float(primary_extent[0]), float(extent[0])):
                cursor_x = 0.
                cursor_y += shelf_height + 20.
                shelf_height = 0.
            if extent[0] > primary_extent[0]:
                cursor_x = 0.
                cursor_y = max(cursor_y, float(primary_extent[1]) + 20.)
            offset = np.asarray([cursor_x, cursor_y]) - chart['low']
            cursor_x += float(extent[0]) + 20.
            shelf_height = max(shelf_height, float(extent[1]))
        for index in chart['ids']:
            triangles[index]['paintUv'] = (mapped_uv[index] + offset).astype(np.float32).tolist()
            triangles[index]['chartId'] = f'{triangles[index]["partId"]}/source-uv0-chart/{ordinal:03d}'
    precision_fallbacks = 0
    cursor_x = 0.
    cursor_y = max(float(np.asarray(t['paintUv']).max(axis=0)[1]) for t in triangles) + 20.
    shelf_height = 0.
    for index, triangle in enumerate(triangles):
        uv = np.asarray(triangle['paintUv'], dtype=np.float32)
        edge_a, edge_b = uv[1] - uv[0], uv[2] - uv[0]
        determinant = float(edge_a[0] * edge_b[1] - edge_a[1] * edge_b[0])
        if abs(determinant) >= 1e-12:
            continue
        points = np.asarray(triangle['world'], dtype=np.float64)
        pairs = ((0, 1), (1, 2), (2, 0))
        a, b = max(pairs, key=lambda pair: np.linalg.norm(points[pair[1]] - points[pair[0]]))
        c = 3 - a - b
        direction = points[b] - points[a]
        length = float(np.linalg.norm(direction))
        if length <= 1e-12:
            raise ValueError('Degenerate world triangle in legacy source UV fallback')
        other = points[c] - points[a]
        height = float(np.linalg.norm(np.cross(direction, other)) / length)
        along = float(np.dot(other, direction / length))
        chart_height_mm = max(height * 1000., 0.1)
        extent = np.asarray([max(length, abs(along)) * 1000., chart_height_mm])
        if cursor_x > 0 and cursor_x + extent[0] > max(float(primary_extent[0]), float(extent[0])):
            cursor_x = 0.
            cursor_y += shelf_height + 20.
            shelf_height = 0.
        if extent[0] > primary_extent[0]:
            cursor_x = 0.
            cursor_y = max(cursor_y, float(primary_extent[1]) + 20.)
        local = np.zeros((3, 2), dtype=np.float64)
        local[a] = [cursor_x, cursor_y]
        local[b] = [cursor_x + length * 1000., cursor_y]
        local[c] = [cursor_x + along * 1000., cursor_y + chart_height_mm]
        fallback = local.astype(np.float32)
        edge_a, edge_b = fallback[1] - fallback[0], fallback[2] - fallback[0]
        if abs(float(edge_a[0] * edge_b[1] - edge_a[1] * edge_b[0])) < 1e-12:
            raise ValueError('Intrinsic fallback collapsed after float32 conversion')
        triangle['paintUv'] = fallback.tolist()
        triangle['chartId'] = f'{triangle["partId"]}/source-uv0-precision/{precision_fallbacks:03d}'
        cursor_x += float(extent[0]) + 20.
        shelf_height = max(shelf_height, float(extent[1]))
        precision_fallbacks += 1
    all_points = np.asarray([t['paintUv'] for t in triangles], dtype=np.float64).reshape((-1, 2))
    return {
        'triangles': len(triangles),
        'sourceMeshes': sorted({t['id']['sourceMesh'] for t in triangles}),
        'sourceUvIslands': len(chart_data),
        'worldAreaM2': float(world_areas.sum()),
        'sourceUvAreaPx2': float(uv_area.sum()),
        'equalAreaScaleMmPerPixel': scale,
        'precisionFallbackTriangles': precision_fallbacks,
        'dimensionsMm': (all_points.max(axis=0) - all_points.min(axis=0)).tolist(),
    }


def pack_islands(part: str, triangles: list[dict], gap_mm: float = 20.) -> dict:
    components = islands(triangles)
    coords = np.asarray([t['paintUv'] for t in triangles], dtype=np.float64)
    weights = area3(np.asarray([t['world'] for t in triangles], dtype=np.float64))
    details = []
    for ids in components:
        points = coords[ids].reshape((-1, 2))
        bounds = (points.min(axis=0), points.max(axis=0))
        details.append({'ids': ids, 'bounds': bounds, 'area': float(weights[ids].sum())})
    details.sort(key=lambda item: (-item['area'], tuple(item['bounds'][0])))
    main = details[0]
    main_width, main_height = main['bounds'][1] - main['bounds'][0]
    main_origin = main['bounds'][0]
    for item in main['ids']:
        triangles[item]['paintUv'] = (coords[item] - main_origin).astype(np.float32).tolist()
    for ordinal, component in enumerate(details):
        if component is main:
            name = f'{part}/classified-island/000'
            for index in component['ids']:
                triangles[index]['chartId'] = name
            continue
        points = coords[component['ids']].reshape((-1, 2))
        low = points.min(axis=0)
        high = points.max(axis=0)
        extent = high - low
        # Each disconnected source island is a rigid chart; stack narrow strips
        # below the primary panel and wrap wider shapes to a new shelf.
        if ordinal == 1:
            cursor_x, cursor_y, shelf_height = 0., main_height + gap_mm, 0.
        if cursor_x > 0 and cursor_x + extent[0] > max(main_width, extent[0]):
            cursor_x = 0.
            cursor_y += shelf_height + gap_mm
            shelf_height = 0.
        if extent[0] > main_width:
            cursor_x = 0.
            cursor_y = max(cursor_y, main_height + gap_mm)
        offset = np.asarray([cursor_x, cursor_y]) - low
        for index in component['ids']:
            triangles[index]['paintUv'] = (coords[index] + offset).astype(np.float32).tolist()
            triangles[index]['chartId'] = f'{part}/classified-island/{ordinal:03d}'
        cursor_x += float(extent[0]) + gap_mm
        shelf_height = max(shelf_height, float(extent[1]))
    all_points = np.asarray([t['paintUv'] for t in triangles]).reshape((-1, 2))
    return {
        'triangleCount': len(triangles),
        'islandCount': len(details),
        'mainIslandWorldAreaM2': main['area'],
        'supplementWorldAreaM2': sum(item['area'] for item in details[1:]),
        'sourceIslandBoundsMm': [float(v) for v in (all_points.max(axis=0) - all_points.min(axis=0))],
        'islands': [
            {'triangles': len(item['ids']), 'worldAreaM2': item['area'],
             'originalBoundsMm': [float(v) for v in (item['bounds'][1] - item['bounds'][0])]}
            for item in details
        ],
    }


def render_preview(groups: dict[str, list[dict]], path: Path):
    font_path = Path('C:/Windows/Fonts/msyh.ttc')
    font = ImageFont.truetype(str(font_path), 16) if font_path.exists() else ImageFont.load_default()
    columns, cell_w, cell_h = 4, 480, 360
    rows = math.ceil(len(groups) / columns)
    image = Image.new('RGB', (columns * cell_w, rows * cell_h), (25, 28, 32))
    draw = ImageDraw.Draw(image)
    palette = [(67, 165, 218), (232, 130, 85), (120, 191, 111), (191, 133, 218),
               (223, 188, 76), (80, 194, 163), (228, 113, 151)]
    for number, (part, triangles) in enumerate(sorted(groups.items())):
        points = np.asarray([t['paintUv'] for t in triangles], dtype=np.float64).reshape((-1, 2))
        low, high = points.min(axis=0), points.max(axis=0)
        scale = min((cell_w - 28) / max(1., high[0] - low[0]),
                    (cell_h - 44) / max(1., high[1] - low[1]))
        offset = np.asarray([(number % columns) * cell_w + 14,
                             (number // columns) * cell_h + 30])
        for triangle in triangles:
            uv = np.asarray(triangle['paintUv'], dtype=np.float64)
            polygon = np.rint((uv - low) * scale + offset).astype(int)
            draw.polygon([tuple(p) for p in polygon], fill=palette[number % len(palette)])
        draw.text(((number % columns) * cell_w + 10, (number // columns) * cell_h + 7),
                  part, fill=(238, 240, 244), font=font)
    image.save(path)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--input', type=Path, default=DEFAULT_INPUT)
    parser.add_argument('--model', type=Path, default=DEFAULT_MODEL)
    parser.add_argument('--output', type=Path, default=DEFAULT_OUTPUT)
    parser.add_argument('--atlas-width', type=int, default=2048)
    parser.add_argument('--atlas-height', type=int, default=2048)
    parser.add_argument('--include-unclassified-source-uv0', action='store_true',
                        help='Explicitly retain legacy parts without a classified PNG/PSD mask.')
    options = parser.parse_args()
    options.output.mkdir(parents=True, exist_ok=False)
    document = json.loads(options.input.read_text(encoding='utf-8'))
    glb = load_glb(options.model)
    groups: dict[str, list[dict]] = defaultdict(list)
    for triangle in document['triangles']:
        groups[triangle['partId']].append(triangle)
    source_report = json.loads((options.input.parent / 'build-report.json').read_text(encoding='utf-8'))
    classified = {part['partId'] for part in source_report['parts']
                  if part.get('status', '').startswith('candidate-mask-clipped')}
    unclassified = sorted(set(groups) - classified)
    excluded = []
    if unclassified and not options.include_unclassified_source_uv0:
        for part in unclassified:
            excluded.append({'partId': part, 'triangleCount': len(groups.pop(part)),
                             'reason': 'no corresponding user-classified PNG/PSD mask'})
        included_ids = {
            tuple(triangle['id'][key] for key in ('sourceMesh', 'primitive', 'sourceTriangle', 'fragment'))
            for triangles in groups.values() for triangle in triangles
        }
        document['triangles'] = [
            triangle for triangle in document['triangles']
            if tuple(triangle['id'][key] for key in ('sourceMesh', 'primitive', 'sourceTriangle', 'fragment'))
            in included_ids
        ]
        def connection_triangle_id(endpoint):
            triangle = endpoint.get('triangle', {})
            return tuple(triangle.get(key) for key in ('sourceMesh', 'primitive', 'sourceTriangle', 'fragment'))
        original_connections = len(document.get('connections', []))
        document['connections'] = [
            connection for connection in document.get('connections', [])
            if connection_triangle_id(connection.get('from', {})) in included_ids
            and connection_triangle_id(connection.get('to', {})) in included_ids
        ]
        removed_connections = original_connections - len(document['connections'])
    else:
        removed_connections = 0
    report = {
        'status': 'packed-classified-surface-map-candidate',
        'classifiedParts': sorted(classified),
        'excludedUnclassifiedParts': excluded,
        'unclassifiedParts': sorted(set(groups) - classified),
        'includedTriangleWorldGeometryAndSourceLineageUnchanged': True,
        'unclassifiedSourceUv0FallbackEnabled': options.include_unclassified_source_uv0,
        'connectionsRemovedWithUnclassifiedParts': removed_connections,
        'classificationAlphaIslandsRemoved': 0,
        'warnings': [
            'Disconnected user-classified islands are retained and rigidly arranged near their primary island.',
            'Only user-classified PNG/PSD parts are included unless the legacy fallback flag is explicitly enabled.',
        ],
        'parts': [],
    }
    atlas_size = (options.atlas_width, options.atlas_height)
    for part, triangles in groups.items():
        if part in classified:
            entry = {'partId': part, 'status': 'classified-alpha-preserved', **pack_islands(part, triangles)}
        else:
            entry = {'partId': part, 'status': 'unclassified-source-uv0-restored',
                     **restore_source_uv(triangles, glb, atlas_size)}
        report['parts'].append(entry)
        print(part, entry['status'], entry.get('islandCount', entry.get('sourceUvIslands')),
              entry.get('sourceIslandBoundsMm', entry.get('dimensionsMm')), flush=True)
    document['paintFingerprint'] = ''
    document['triangles'].sort(key=lambda t: (t['partId'], t['id']['sourceMesh'],
                                               t['id']['sourceTriangle'], t['id']['fragment']))
    (options.output / 'paint-map-unsealed.json').write_text(
        json.dumps(document, separators=(',', ':')), encoding='utf-8')
    render_preview(groups, options.output / 'parts-preview.png')
    report['parts'].sort(key=lambda entry: entry['partId'])
    (options.output / 'build-report.json').write_text(
        json.dumps(report, ensure_ascii=False, indent=2), encoding='utf-8')


if __name__ == '__main__':
    main()
