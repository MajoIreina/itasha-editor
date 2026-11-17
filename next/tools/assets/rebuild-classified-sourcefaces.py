"""Build an unsealed source-face paint map clipped to the user's part masks.

The existing paint map supplies only an initial part-to-source-mesh association.
Every candidate source face is reconstructed from the GLB, intersected with the
classified alpha silhouette in UV0, and written with reversible barycentrics.
PSD registrations with nonzero RGB residuals stay explicitly unapproved.
"""
from __future__ import annotations

import argparse
import hashlib
import importlib.util
import json
import math
from pathlib import Path
import sys

import cv2
import numpy as np
from PIL import Image, ImageDraw
from shapely import constrained_delaunay_triangles, make_valid
from shapely.geometry import GeometryCollection, MultiPolygon, Polygon
from shapely.ops import unary_union

from classified_paint_masks import load_classified_masks


NEXT = Path(__file__).resolve().parents[2]
WORKSPACE = NEXT.parents[1]
DEFAULT_INPUT = NEXT / 'assets/paint/su7-v16-classified-unfolding/paint-map.json'
DEFAULT_OUTPUT = NEXT / 'assets/paint/su7-v18b-classified-sourcefaces'
DEFAULT_MODEL = WORKSPACE / '3D/Blender/software/body.glb'


def load_glb_reader():
    path = NEXT / 'tools/assets/audit-classified-uv.py'
    spec = importlib.util.spec_from_file_location('audit_classified_uv_reader', path)
    if spec is None or spec.loader is None:
        raise RuntimeError(f'Cannot load GLB reader: {path}')
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module.Glb


def polygons(geometry):
    if isinstance(geometry, Polygon):
        return [geometry] if geometry.area > 1e-7 else []
    if isinstance(geometry, (MultiPolygon, GeometryCollection)):
        result = []
        for child in geometry.geoms:
            result.extend(polygons(child))
        return result
    return []


def mask_geometry(mask: np.ndarray):
    binary = np.pad(mask.astype(np.uint8), 1)
    contours, hierarchy = cv2.findContours(binary, cv2.RETR_CCOMP, cv2.CHAIN_APPROX_SIMPLE)
    if hierarchy is None:
        return GeometryCollection()
    hierarchy = hierarchy[0]
    rings = [np.asarray(contour, dtype=np.float64).reshape(-1, 2) - 1.
             for contour in contours]
    parts = []
    for index, item in enumerate(hierarchy):
        if item[3] >= 0:
            continue
        outer = rings[index]
        if len(outer) < 3:
            continue
        holes = []
        child = item[2]
        while child >= 0:
            if len(rings[child]) >= 3:
                holes.append(rings[child])
            child = hierarchy[child][0]
        poly = Polygon(outer, holes)
        if not poly.is_valid:
            poly = make_valid(poly)
        parts.extend(polygons(poly))
    # Contours follow pixel centers; a half-pixel dilation recovers the occupied
    # pixel-cell boundary while retaining holes and the classified silhouette.
    return unary_union([part.buffer(.5, join_style='mitre') for part in parts])


def barycentric_from_uv(points: np.ndarray, triangle: np.ndarray) -> np.ndarray:
    a = triangle[1] - triangle[0]
    b = triangle[2] - triangle[0]
    det = a[0] * b[1] - a[1] * b[0]
    if abs(det) < 1e-9:
        raise ValueError('Cannot map a clipped point through a degenerate source UV triangle')
    delta = points - triangle[0]
    second = (delta[:, 0] * b[1] - delta[:, 1] * b[0]) / det
    third = (a[0] * delta[:, 1] - a[1] * delta[:, 0]) / det
    return np.column_stack((1. - second - third, second, third))


def fragment_id(mesh: str, face: int, part: str, number: int) -> int:
    token = f'{mesh}|{face}|{part}|classified|{number}'.encode('utf-8')
    return int.from_bytes(hashlib.sha256(token).digest()[:8], 'little')


def area2(triangles: np.ndarray) -> np.ndarray:
    a = triangles[:, 1] - triangles[:, 0]
    b = triangles[:, 2] - triangles[:, 0]
    return np.abs(a[:, 0] * b[:, 1] - a[:, 1] * b[:, 0]) * .5


def area3(triangles: np.ndarray) -> np.ndarray:
    return np.linalg.norm(np.cross(triangles[:, 1] - triangles[:, 0],
                                   triangles[:, 2] - triangles[:, 0]), axis=1) * .5


def cross2(a: np.ndarray, b: np.ndarray) -> float:
    return float(a[0] * b[1] - a[1] * b[0])


def rasterize(uv: np.ndarray, size: tuple[int, int], supersample: int = 1) -> np.ndarray:
    width, height = size
    image = Image.new('1', (width * supersample, height * supersample), 0)
    draw = ImageDraw.Draw(image)
    for triangle in uv:
        draw.polygon([tuple(np.rint(p * supersample).astype(int)) for p in triangle], fill=1)
    pixels = np.asarray(image, dtype=bool)
    if supersample == 1:
        return pixels
    return pixels.reshape(height, supersample, width, supersample).mean(axis=(1, 3)) >= .5


def put_preview(target: Image.Image, mask: np.ndarray, color: tuple[int, int, int]):
    target.paste(Image.new('RGB', target.size, color), (0, 0),
                 Image.fromarray(mask.astype(np.uint8) * 255))


def identity(triangle: dict) -> tuple:
    value = triangle['id']
    return tuple(value[k] for k in ('sourceMesh', 'primitive', 'sourceTriangle', 'fragment'))


def recover_connections(source_map: dict, triangles: list[dict]):
    """Rebuild clipped seam links from exact source-face barycentric lineage."""
    source_by_id = {identity(t): t for t in source_map['triangles']}
    output_by_id = {identity(t): t for t in triangles}

    def lineage_key(triangle):
        tid = triangle['id']
        return (triangle['partId'], tid['sourceMesh'], tid['primitive'], tid['sourceTriangle'])

    edges_by_lineage = {}
    for triangle in triangles:
        bary = np.asarray(triangle['sourceBarycentric'], dtype=np.float64)
        world = np.asarray(triangle['world'], dtype=np.float64)
        for edge in range(3):
            edges_by_lineage.setdefault(lineage_key(triangle), []).append({
                'triangleId': identity(triangle),
                'edge': edge,
                'bary': np.asarray([bary[edge], bary[(edge + 1) % 3]]),
                'world': np.asarray([world[edge], world[(edge + 1) % 3]]),
            })

    def candidates(old_triangle, edge, reversed_order):
        bary = np.asarray(old_triangle['sourceBarycentric'], dtype=np.float64)
        world = np.asarray(old_triangle['world'], dtype=np.float64)
        p0, p1 = bary[edge], bary[(edge + 1) % 3]
        w0, w1 = world[edge], world[(edge + 1) % 3]
        direction = p1 - p0
        denominator = float(np.dot(direction, direction))
        if denominator < 1e-14:
            return []
        result = []
        for entry in edges_by_lineage.get(lineage_key(old_triangle), []):
            q0, q1 = entry['bary']
            t0 = float(np.dot(q0 - p0, direction) / denominator)
            t1 = float(np.dot(q1 - p0, direction) / denominator)
            residual0 = float(np.linalg.norm(q0 - (p0 + direction * t0)))
            residual1 = float(np.linalg.norm(q1 - (p0 + direction * t1)))
            if max(residual0, residual1) > 2e-5 or abs(t1 - t0) < 1e-8:
                continue
            expected0 = w0 + (w1 - w0) * t0
            expected1 = w0 + (w1 - w0) * t1
            if max(float(np.linalg.norm(entry['world'][0] - expected0)),
                   float(np.linalg.norm(entry['world'][1] - expected1))) > 1e-4:
                continue
            c0, c1 = ((1. - t0, 1. - t1) if reversed_order else (t0, t1))
            low, high = max(0., min(c0, c1)), min(1., max(c0, c1))
            if high - low <= 1e-7:
                continue
            result.append({
                **entry,
                'common0': c0,
                'common1': c1,
                'low': low,
                'high': high,
            })
        return result

    def local_parameter(entry, common_t):
        return (common_t - entry['common0']) / (entry['common1'] - entry['common0'])

    def edge_value(entry, common_t, attribute):
        local = local_parameter(entry, common_t)
        return entry[attribute][0] + (entry[attribute][1] - entry[attribute][0]) * local

    retained = []
    plans = []
    split_parameters: dict[tuple, dict[int, list[float]]] = {}
    used_ids = set()
    retained_source_ids = set()
    partial_source_ids = set()
    planned_segments_by_source: dict[str, int] = {}
    report = {
        'input': len(source_map.get('connections', [])),
        'retained': 0,
        'matchedConnections': 0,
        'generatedSegments': 0,
        'partiallyCoveredConnections': 0,
        'unresolvedConnections': 0,
        'lineageCandidatesRejectedByGapOrLength': 0,
    }

    for connection in source_map.get('connections', []):
        from_id = tuple(connection['from']['triangle'][k]
                        for k in ('sourceMesh', 'primitive', 'sourceTriangle', 'fragment'))
        to_id = tuple(connection['to']['triangle'][k]
                      for k in ('sourceMesh', 'primitive', 'sourceTriangle', 'fragment'))
        if from_id in output_by_id and to_id in output_by_id:
            retained.append(connection)
            used_ids.add(connection['id'])
            retained_source_ids.add(connection['id'])
            report['retained'] += 1
            continue
        old_from, old_to = source_by_id.get(from_id), source_by_id.get(to_id)
        if old_from is None or old_to is None:
            report['unresolvedConnections'] += 1
            continue
        from_edge, to_edge = int(connection['from']['edge']), int(connection['to']['edge'])
        left = candidates(old_from, from_edge, False)
        right = candidates(old_to, to_edge, bool(connection['reversed']))
        if not left or not right:
            report['unresolvedConnections'] += 1
            continue

        points = {0., 1.}
        for entry in left + right:
            points.add(entry['low'])
            points.add(entry['high'])
        points = sorted(points)
        segments = []
        for start, end in zip(points, points[1:]):
            if end - start <= 1e-7:
                continue
            middle = (start + end) * .5
            left_cover = [e for e in left if e['low'] - 1e-7 <= middle <= e['high'] + 1e-7]
            right_cover = [e for e in right if e['low'] - 1e-7 <= middle <= e['high'] + 1e-7]
            choices = []
            for lentry in left_cover:
                for rentry in right_cover:
                    la = edge_value(lentry, start, 'world')
                    lb = edge_value(lentry, end, 'world')
                    ra = edge_value(rentry, start, 'world')
                    rb = edge_value(rentry, end, 'world')
                    gap = max(float(np.linalg.norm(la - ra)), float(np.linalg.norm(lb - rb)))
                    length_error = abs(float(np.linalg.norm(lb - la) - np.linalg.norm(rb - ra)))
                    if (gap > float(connection['physicalGapMm']) / 1000. + .0049 or
                            length_error > .0049):
                        continue
                    key = (lentry['triangleId'], lentry['edge'], rentry['triangleId'], rentry['edge'])
                    choices.append((gap + length_error, key, lentry, rentry, gap))
            if not choices:
                if left_cover and right_cover:
                    report['lineageCandidatesRejectedByGapOrLength'] += 1
                continue
            _, _, lentry, rentry, gap = min(choices, key=lambda item: item[0:2])
            segment = {'start': start, 'end': end, 'left': lentry, 'right': rentry, 'gap': gap}
            if (segments and segments[-1]['left'] is lentry and segments[-1]['right'] is rentry and
                    abs(segments[-1]['end'] - start) <= 1e-7):
                previous = segments[-1]
                la = edge_value(lentry, previous['start'], 'world')
                lb = edge_value(lentry, end, 'world')
                ra = edge_value(rentry, previous['start'], 'world')
                rb = edge_value(rentry, end, 'world')
                gap = max(float(np.linalg.norm(la - ra)), float(np.linalg.norm(lb - rb)))
                length_error = abs(float(np.linalg.norm(lb - la) - np.linalg.norm(rb - ra)))
                if (gap <= float(connection['physicalGapMm']) / 1000. + .0049 and
                        length_error <= .0049):
                    previous['end'] = end
                    previous['gap'] = gap
                    continue
            segments.append(segment)

        if not segments:
            report['unresolvedConnections'] += 1
            continue
        covered = sum(segment['end'] - segment['start'] for segment in segments)
        if covered < .999:
            partial_source_ids.add(connection['id'])
        planned_segments_by_source[connection['id']] = len(segments)

        for segment_index, segment in enumerate(segments):
            start, end = segment['start'], segment['end']
            lentry, rentry = segment['left'], segment['right']
            ls = (local_parameter(lentry, start), local_parameter(lentry, end))
            rs = (local_parameter(rentry, start), local_parameter(rentry, end))
            split_parameters.setdefault(lentry['triangleId'], {}).setdefault(
                lentry['edge'], []).extend(ls)
            split_parameters.setdefault(rentry['triangleId'], {}).setdefault(
                rentry['edge'], []).extend(rs)
            plans.append({
                'sourceId': connection['id'],
                'id': f"{connection['id']}/classified-sourceface/{segment_index:03d}",
                'fromParent': lentry['triangleId'],
                'fromEdge': lentry['edge'],
                'fromBaryPoints': [edge_value(lentry, start, 'bary').astype(np.float32),
                                   edge_value(lentry, end, 'bary').astype(np.float32)],
                'toParent': rentry['triangleId'],
                'toEdge': rentry['edge'],
                'toBaryPoints': [edge_value(rentry, start, 'bary').astype(np.float32),
                                 edge_value(rentry, end, 'bary').astype(np.float32)],
                'physicalGapMm': segment['gap'] * 1000.,
                'propagation': bool(connection['propagation']),
            })

    children = {}
    split_triangles = []
    for triangle in triangles:
        parent_id = identity(triangle)
        edge_splits = split_parameters.get(parent_id)
        if not edge_splits:
            children[parent_id] = [triangle]
            split_triangles.append(triangle)
            continue
        ring = []
        world = np.asarray(triangle['world'], dtype=np.float64)
        bary = np.asarray(triangle['sourceBarycentric'], dtype=np.float64)
        uv = np.asarray(triangle['paintUv'], dtype=np.float64)
        for edge in range(3):
            start = edge
            end = (edge + 1) % 3
            ring.append((world[start], bary[start], uv[start]))
            values = sorted(max(0., min(1., t)) for t in edge_splits.get(edge, [])
                            if 1e-6 < t < 1. - 1e-6)
            compact = []
            for value in values:
                if not compact or abs(value - compact[-1]) > 1e-6:
                    compact.append(value)
            for t in compact:
                ring.append((world[start] * (1. - t) + world[end] * t,
                             bary[start] * (1. - t) + bary[end] * t,
                             uv[start] * (1. - t) + uv[end] * t))
        center = (world.mean(axis=0), bary.mean(axis=0), uv.mean(axis=0))
        generated = []
        for index in range(len(ring)):
            vertices = [center, ring[index], ring[(index + 1) % len(ring)]]
            child = {k: v for k, v in triangle.items()}
            child['id'] = dict(triangle['id'])
            token = f"{triangle['id']['fragment']}|seam-split|{index}".encode('utf-8')
            child['id']['fragment'] = int.from_bytes(hashlib.sha256(token).digest()[:8], 'little')
            child['world'] = np.asarray([v[0] for v in vertices], dtype=np.float32).tolist()
            child['sourceBarycentric'] = np.asarray([v[1] for v in vertices], dtype=np.float32).tolist()
            child['paintUv'] = np.asarray([v[2] for v in vertices], dtype=np.float32).tolist()
            generated.append(child)
            split_triangles.append(child)
        children[parent_id] = generated

    def find_child_edge(parent_id, a, b):
        a, b = np.asarray(a, dtype=np.float64), np.asarray(b, dtype=np.float64)
        for child in children.get(parent_id, []):
            points = np.asarray(child['sourceBarycentric'], dtype=np.float64)
            for edge in range(3):
                p0, p1 = points[edge], points[(edge + 1) % 3]
                direct = max(float(np.linalg.norm(p0 - a)), float(np.linalg.norm(p1 - b)))
                reverse = max(float(np.linalg.norm(p0 - b)), float(np.linalg.norm(p1 - a)))
                if min(direct, reverse) <= 2e-6:
                    return child, edge
        return None

    rebuilt = retained[:]
    successful_source_ids = set()
    successful_segments_by_source: dict[str, int] = {}
    for plan in plans:
        left = find_child_edge(plan['fromParent'], *plan['fromBaryPoints'])
        right = find_child_edge(plan['toParent'], *plan['toBaryPoints'])
        if left is None or right is None:
            continue
        lt, le = left
        rt, re = right
        la, lb = (np.asarray(lt['world'][le]), np.asarray(lt['world'][(le + 1) % 3]))
        ra, rb = (np.asarray(rt['world'][re]), np.asarray(rt['world'][(re + 1) % 3]))
        direct = np.linalg.norm(la - ra) + np.linalg.norm(lb - rb)
        reverse = np.linalg.norm(la - rb) + np.linalg.norm(lb - ra)
        reversed_order = reverse < direct
        aligned = (rb, ra) if reversed_order else (ra, rb)
        physical_gap = max(float(np.linalg.norm(la - aligned[0])),
                           float(np.linalg.norm(lb - aligned[1]))) * 1000.
        length_error = abs(float(np.linalg.norm(lb - la) - np.linalg.norm(rb - ra)))
        if physical_gap > plan['physicalGapMm'] + .001 or length_error > .005:
            continue
        connection_id = plan['id']
        if connection_id in used_ids:
            connection_id += '/' + hashlib.sha256(connection_id.encode('utf-8')).hexdigest()[:12]
        used_ids.add(connection_id)
        rebuilt.append({
            'id': connection_id,
            'from': {'triangle': lt['id'], 'edge': le},
            'to': {'triangle': rt['id'], 'edge': re},
            'reversed': bool(reversed_order),
            'physicalGapMm': physical_gap,
            'propagation': plan['propagation'],
        })
        successful_source_ids.add(plan['sourceId'])
        successful_segments_by_source[plan['sourceId']] = (
            successful_segments_by_source.get(plan['sourceId'], 0) + 1
        )
    report['generatedSegments'] = len(rebuilt) - len(retained)
    report['matchedConnections'] = len(successful_source_ids)
    report['unresolvedConnections'] = max(
        0, report['input'] - len(retained_source_ids) - len(successful_source_ids)
    )
    failed_segments = {
        source_id for source_id, planned in planned_segments_by_source.items()
        if successful_segments_by_source.get(source_id, 0) < planned
    }
    report['partiallyCoveredConnections'] = len(partial_source_ids | failed_segments)
    return split_triangles, rebuilt, report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--input', type=Path, default=DEFAULT_INPUT)
    parser.add_argument('--model', type=Path, default=DEFAULT_MODEL)
    parser.add_argument('--output', type=Path, default=DEFAULT_OUTPUT)
    parser.add_argument('--min-mask-area-px2', type=float, default=.02)
    options = parser.parse_args()
    if options.output.exists():
        raise FileExistsError(f'Refusing to overwrite existing candidate: {options.output}')
    if options.min_mask_area_px2 < 0:
        raise ValueError('--min-mask-area-px2 must be nonnegative')

    masks, registration = load_classified_masks()
    source_map = json.loads(options.input.read_text(encoding='utf-8'))
    glb = load_glb_reader()(options.model)
    model_hash = hashlib.sha256(options.model.read_bytes()).hexdigest()
    if source_map.get('modelFingerprint') != model_hash:
        raise ValueError('Input paint map and GLB fingerprints differ')

    grouped: dict[str, list[dict]] = {}
    for triangle in source_map['triangles']:
        grouped.setdefault(triangle['partId'], []).append(triangle)
    document = {k: v for k, v in source_map.items()}
    output_triangles = []
    part_reports = []
    preview_order = list(masks)
    palette = [(235, 90, 66), (80, 176, 238), (255, 205, 70), (176, 111, 222),
               (74, 203, 165), (242, 146, 66), (85, 132, 225), (235, 110, 160),
               (125, 202, 95), (229, 157, 76), (112, 191, 220), (208, 101, 98),
               (105, 154, 218), (242, 202, 92), (185, 126, 226), (85, 194, 158),
               (237, 127, 196)]
    size = (int(registration['atlasPx'][0]), int(registration['atlasPx'][1]))
    classified_preview = Image.new('RGB', size, (22, 24, 28))
    geometry_preview = Image.new('RGB', size, (22, 24, 28))
    registration_by_part = {item.get('partId'): item for item in registration.get('sources', [])}

    for part, mask in masks.items():
        existing = grouped.get(part)
        if not existing:
            part_reports.append({'partId': part, 'status': 'mask-without-source-assignment'})
            continue
        meshes = {t['id']['sourceMesh'] for t in existing}
        if len(meshes) != 1:
            part_reports.append({'partId': part, 'status': 'ambiguous-source-mesh',
                                 'sourceMeshes': sorted(meshes)})
            continue
        mesh_id = next(iter(meshes))
        geometry = glb.geometry(mesh_id)
        face_uv = geometry['uv'].astype(np.float64) * np.asarray(size, dtype=np.float64)
        face_world = geometry['world'].astype(np.float64)
        mask_shape = mask_geometry(mask)
        min_x, min_y, max_x, max_y = mask_shape.bounds
        color = palette[preview_order.index(part) % len(palette)]
        put_preview(classified_preview, mask, color)

        candidate_polys = []
        uv_areas = []
        world_areas = []
        face_clip_counts = {}
        for face, source_uv in enumerate(face_uv):
            if abs(cross2(source_uv[1] - source_uv[0], source_uv[2] - source_uv[0])) < 2e-7:
                continue
            low = source_uv.min(axis=0)
            high = source_uv.max(axis=0)
            if high[0] < min_x or high[1] < min_y or low[0] > max_x or low[1] > max_y:
                continue
            source_poly = Polygon(source_uv)
            if not source_poly.intersects(mask_shape):
                continue
            clipped = source_poly.intersection(mask_shape)
            face_tris = []
            for polygon in polygons(clipped):
                if polygon.area < options.min_mask_area_px2:
                    continue
                triangulated = constrained_delaunay_triangles(polygon)
                for output_poly in polygons(triangulated):
                    points = np.asarray(output_poly.exterior.coords[:-1], dtype=np.float64)
                    if len(points) != 3:
                        continue
                    if abs(cross2(points[1] - points[0], points[2] - points[0])) < 2e-7:
                        continue
                    face_tris.append(points)
            if face_tris:
                face_clip_counts[face] = len(face_tris)
                candidate_polys.extend((face, tri) for tri in face_tris)

        if not candidate_polys:
            raise ValueError(f'{part}: no source surface intersects its classified mask')
        uv_triangles = np.asarray([tri for _, tri in candidate_polys], dtype=np.float64)
        uv_area = area2(uv_triangles)
        bcoords = [barycentric_from_uv(tri, face_uv[face]) for face, tri in candidate_polys]
        barycentrics = np.asarray(bcoords, dtype=np.float64)
        world_triangles = np.asarray([
            bary @ face_world[face] for (face, _), bary in zip(candidate_polys, barycentrics)
        ], dtype=np.float64)
        world_area = area3(world_triangles)
        valid = (uv_area > 1e-7) & (world_area > 1e-12)
        uv_triangles, uv_area = uv_triangles[valid], uv_area[valid]
        world_triangles, world_area = world_triangles[valid], world_area[valid]
        barycentrics = barycentrics[valid]
        candidate_polys = [item for item, keep in zip(candidate_polys, valid) if keep]
        scale = math.sqrt(float(world_area.sum()) * 1e6 / float(uv_area.sum()))
        origin = np.asarray([min_x, min_y], dtype=np.float64)
        part_triangles = []
        per_face_ordinal: dict[int, int] = {}
        for (face, _), uv, world, bary in zip(candidate_polys, uv_triangles, world_triangles, barycentrics):
            ordinal = per_face_ordinal.get(face, 0)
            per_face_ordinal[face] = ordinal + 1
            source_identity = existing[0]['id']
            triangle = {
                'id': {
                    'sourceMesh': mesh_id,
                    'primitive': int(source_identity['primitive']),
                    'sourceTriangle': int(face),
                    'fragment': fragment_id(mesh_id, face, part, ordinal),
                },
                'partId': part,
                'regionId': part + '/classified-exterior',
                'chartId': part + '/classified-source-uv0',
                'world': world.astype(np.float32).tolist(),
                'sourceBarycentric': bary.astype(np.float32).tolist(),
                'paintUv': ((uv - origin) * scale).astype(np.float32).tolist(),
            }
            part_triangles.append(triangle)
        output_triangles.extend(part_triangles)
        raster = rasterize(uv_triangles, size)
        intersection = int(np.count_nonzero(raster & mask))
        geometry_pixels = int(raster.sum())
        mask_pixels = int(mask.sum())
        detail = registration_by_part.get(part, {})
        status = 'candidate-mask-clipped'
        if detail.get('kind') == 'psd-layer' and (
            detail.get('rgbExactPixelFraction', 0) < .999 or
            detail.get('detailRgbMeanAbsoluteError', 0) > .5
        ):
            status = 'candidate-mask-clipped-registration-review-required'
        put_preview(geometry_preview, raster, color)
        part_reports.append({
            'partId': part,
            'status': status,
            'sourceMesh': mesh_id,
            'sourcePrimitiveTriangleCount': int(len(face_uv)),
            'sourceFacesIntersectingMask': len(face_clip_counts),
            'clippedTriangleCount': len(part_triangles),
            'sourceFacesWithMultipleClipTriangles': sum(n > 1 for n in face_clip_counts.values()),
            'maskAlphaPixels': mask_pixels,
            'geometryRasterPixels': geometry_pixels,
            'intersectionPixels': intersection,
            'maskPrecision': intersection / max(geometry_pixels, 1),
            'maskRecall': intersection / max(mask_pixels, 1),
            'maskIoU': intersection / max(mask_pixels + geometry_pixels - intersection, 1),
            'sourceWorldAreaM2': float(world_area.sum()),
            'uvAreaPx2': float(uv_area.sum()),
            'equalAreaScaleMmPerUvPixel': scale,
            'layoutBoundsMm': [float((max_x - min_x) * scale), float((max_y - min_y) * scale)],
            'registration': {
                'kind': detail.get('kind'),
                'source': detail.get('file', detail.get('layer')),
                'rgbExactPixelFraction': detail.get('rgbExactPixelFraction'),
                'detailRgbMeanAbsoluteError': detail.get('detailRgbMeanAbsoluteError'),
                'calibratedOriginPx': detail.get('calibratedOriginPx', detail.get('originPx')),
                'locallyOptimal': detail.get('calibratedOriginLocallyOptimal'),
            },
        })
        print('PART', part, 'triangles', len(part_triangles), 'precision',
              round(intersection / max(geometry_pixels, 1), 4), 'recall',
              round(intersection / max(mask_pixels, 1), 4), 'status', status, flush=True)

    # Parts without a classified source remain exactly as they were; they are not
    # guessed from nearby texture islands or broadcast from the mixed body mesh.
    classified_parts = set(masks)
    retained_existing = [t for t in source_map['triangles'] if t['partId'] not in classified_parts]
    document['triangles'], connections, connection_report = recover_connections(
        source_map, retained_existing + output_triangles)
    document['connections'] = connections
    document['paintFingerprint'] = ''
    document['triangles'].sort(key=lambda t: (t['partId'], t['id']['sourceMesh'],
                                              t['id']['sourceTriangle'], t['id']['fragment']))
    report = {
        'status': 'unsealed-classified-source-face-clipping-candidate',
        'runtimeApproved': False,
        'sourceMap': str(options.input.resolve()),
        'sourceModel': str(options.model.resolve()),
        'modelFingerprint': model_hash,
        'classificationDirectory': registration.get('sources', [{}])[0].get('file', ''),
        'coordinateConvention': 'x=u*width; y=v*height; mask contours in UV0 pixels; one uniform area scale per part',
        'clipping': 'source UV triangle intersected with classified alpha contour, constrained Delaunay triangulation, barycentrics mapped back to GLB world vertices',
        'connectionStatus': connection_report,
        'retainedUnclassifiedParts': sorted({t['partId'] for t in retained_existing}),
        'parts': part_reports,
        'warnings': [
            'PSD source layers with RGB residuals are diagnostic candidates, not approved transforms.',
            'Only exact source-lineage seam intersections are reconnected; unmatched legacy seams remain unresolved.',
            'Unclassified trunk and sill geometry remains unchanged pending explicit source masks.',
            'Candidate has not passed exterior visibility or semantic part-boundary review.',
        ],
    }
    options.output.mkdir(parents=True, exist_ok=False)
    (options.output / 'paint-map-unsealed.json').write_text(
        json.dumps(document, separators=(',', ':')), encoding='utf-8')
    (options.output / 'build-report.json').write_text(
        json.dumps(report, ensure_ascii=False, indent=2), encoding='utf-8')
    preview = Image.new('RGB', (size[0] * 2, size[1] + 30), (14, 16, 19))
    preview.paste(classified_preview, (0, 30))
    preview.paste(geometry_preview, (size[0], 30))
    header = ImageDraw.Draw(preview)
    header.text((10, 10), 'User classified masks', fill=(238, 238, 238))
    header.text((size[0] + 10, 10), 'Source faces clipped to classified alpha', fill=(238, 238, 238))
    preview.save(options.output / 'mask-preview.png')
    print(json.dumps({'triangles': len(document['triangles']), 'connections': len(connections),
                      'output': str(options.output.resolve())}, ensure_ascii=True, indent=2))


if __name__ == '__main__':
    main()
