"""Build an unsealed SU7 repair candidate without modifying models/current maps.

Uses actual left source geometry/UV0 (not mirrored nearest-face identities),
plus independently observed gaps within existing reviewed exterior components.
Inner panels, cowl/rear trim and unclassified components remain explicit pending
regions. This is a candidate builder, not semantic or desktop acceptance.
"""
from __future__ import annotations

import argparse
import copy
from collections import Counter, defaultdict
import hashlib
import json
from pathlib import Path
import runpy

import numpy as np
import shapely
from shapely.geometry import Point, Polygon
from shapely.ops import unary_union


HERE = Path(__file__).resolve().parent
AUDIT = runpy.run_path(str(HERE/'audit-paint-integrity.py'))
LEFT = 'su7.front-left-lower-fender'
RIGHT = 'su7.front-right-lower-fender'
SIMPLEX = Polygon([(0., 0.), (1., 0.), (0., 1.)])


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--model', type=Path, required=True)
    parser.add_argument('--map', type=Path, required=True)
    parser.add_argument('--reviewed', type=Path, required=True)
    parser.add_argument('--audit', type=Path, required=True)
    parser.add_argument('--component-visibility', type=Path, required=True)
    parser.add_argument('--prior-repair',type=Path,help='Retain the exact source union of an independently reviewed earlier repair')
    parser.add_argument('--selection-basis',choices=('normalized-source','reviewed-r4-selection'),default='normalized-source',
                        help='Compatibility selection keeps the independently reviewed r4 target regions; output source areas are always audited after full homogeneous normalization')
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=False)
    current = json.loads(args.map.read_text('utf-8'))
    reviewed = json.loads(args.reviewed.read_text('utf-8'))
    prior_repair=json.loads(args.prior_repair.read_text('utf-8')) if args.prior_repair else None
    model_hash = AUDIT['digest'](args.model)
    for doc in (current, reviewed):
        if doc['modelFingerprint'] != model_hash:
            raise ValueError('Model fingerprint mismatch')
    if prior_repair is not None and prior_repair['modelFingerprint']!=model_hash:
        raise ValueError('Prior reviewed repair model fingerprint mismatch')
    audit = json.loads((args.audit/'geometry.json').read_text('utf-8'))
    recorded_hashes = {v['sha256'] for v in audit['inputs'].values()}
    if AUDIT['digest'](args.map) not in recorded_hashes or model_hash not in recorded_hashes:
        raise ValueError('Audit does not describe these exact inputs')
    visibility = json.loads((args.audit/'visibility.json').read_text('utf-8'))
    full_visibility = json.loads(args.component_visibility.read_text('utf-8'))
    for doc in (visibility, full_visibility):
        if model_hash not in doc['modules'].values():
            raise ValueError('Visibility report does not describe source model')

    glb = AUDIT['Glb'](args.model)
    sources = {p['id']: p for p in glb.primitives()}
    for source in sources.values():
        primitive = glb.document['meshes'][glb.document['nodes'][source['node']]['mesh']]['primitives'][source['primitive']]
        source['uv'] = glb.accessor(primitive['attributes']['TEXCOORD_0']) if 'TEXCOORD_0' in primitive['attributes'] else None
    groups, existing_parts = defaultdict(list), defaultdict(list)
    for triangle in current['triangles']:
        groups[(triangle['id']['sourceMesh'], triangle['id']['sourceTriangle'])].append(triangle)
        existing_parts[triangle['partId']].append(triangle)

    mesh = next(p for p in sources.values() if p['node'] == 175)
    components = AUDIT['components'](mesh['indices'])
    left_faces = next(faces for faces in components if any(np.linalg.norm(mesh['world'][f].mean(axis=0)-[-1.14, .5, .95]) < .06 for f in faces))
    left_faces = set(left_faces)
    visible_faces = {w['face'] for w in full_visibility['witnesses'] if w['sourceMesh'] == mesh['id'] and w['face'] in left_faces}
    old_left = defaultdict(list)
    for t in reviewed['triangles']:
        if t['partId'] == LEFT:
            if t['id']['sourceMesh'] != mesh['id'] or t['id']['sourceTriangle'] not in left_faces:
                raise ValueError('Reviewed left geometry is outside actual component')
            old_left[t['id']['sourceTriangle']].append(t)

    def poly(t):
        return AUDIT['source_polygon'](t)

    def selection_poly(t):
        if args.selection_basis=='reviewed-r4-selection':
            return Polygon(np.asarray(t['sourceBarycentric'],dtype=float)[:,1:])
        return poly(t)

    # Calibrate the common millimetres-per-source-UV scale from the reviewed
    # right chart. A single similarity of the actual left UV0 keeps its shape;
    # nearest faces never determine left geometry, barycentrics, or identities.
    right_chart = Counter(t['chartId'] for t in existing_parts[RIGHT]).most_common(1)[0][0]
    right = [t for t in existing_parts[RIGHT] if t['chartId'] == right_chart]
    fit_x, fit_y = [], []
    for t in right:
        source = sources[t['id']['sourceMesh']]
        uv0 = source['uv'][source['indices'][t['id']['sourceTriangle']]]
        fit_x.extend(np.asarray(t['sourceBarycentric']) @ uv0)
        fit_y.extend(t['paintUv'])
    fit_x = np.c_[np.asarray(fit_x), np.ones(len(fit_x))]
    fit_y = np.asarray(fit_y)
    fit = np.linalg.lstsq(fit_x, fit_y, rcond=None)[0]
    scale = float(np.sqrt(abs(np.linalg.det(fit[:2]))))
    right_fit_error = float(np.linalg.norm(fit_x@fit-fit_y, axis=1).max())
    if right_fit_error > .01:
        raise ValueError('Right chart is not a verified affine UV0 chart')
    left_uv = mesh['uv'].astype(float) @ np.array([[0., -1.], [1., 0.]]) * scale
    left_uv -= left_uv[np.unique(mesh['indices'][sorted(left_faces)])].min(axis=0)

    kept = [t for t in current['triangles'] if t['partId'] != LEFT]
    new_triangles, pending, additions, precision_slivers = [], [], [], []
    source_domain_differences, uv_locator_repairs, degenerate_source_uvs = [], [], []
    next_fragment = max(t['id']['fragment'] for t in current['triangles']) + 1
    chart_fits = {}

    def create(source, face, geometry, part, chart, transform=None):
        nonlocal next_fragment
        original = source['world'][face]
        uv0 = source['uv'][source['indices'][face]]
        pieces = shapely.constrained_delaunay_triangles(geometry)
        made_area = 0.
        skipped_area = 0.
        count = 0
        made_polygons=[]
        for piece in pieces.geoms:
            if piece.area == 0.:
                continue
            coordinates = np.asarray(piece.exterior.coords[:3])
            exact_weights = np.c_[1-coordinates.sum(axis=1), coordinates]
            weights = exact_weights.astype(np.float32)
            normalized=weights.astype(float)/weights.astype(float).sum(axis=1)[:,None]
            for opposite in range(3):
                mask=weights[:,opposite]==0.
                a,b=[i for i in range(3) if i!=opposite]
                normalized[mask,b]=1.-normalized[mask,a]
            world = (normalized @ original).astype(np.float32)
            uv = normalized @ (left_uv[source['indices'][face]] if part == LEFT else uv0)
            if transform is not None:
                uv = np.c_[uv, np.ones(3)] @ transform
            uv = uv.astype(np.float32)
            uv_a, uv_b = uv[1]-uv[0], uv[2]-uv[0]
            uv_cross = float(uv_a[0]*uv_b[1]-uv_a[1]*uv_b[0])
            world64=world.astype(float)
            world_cross=np.cross(world64[1]-world64[0],world64[2]-world64[0])
            if not np.any(world_cross != 0.):
                exact_area = float(piece.area*2*AUDIT['area3'](original))
                precision_slivers.append({'partId': part, 'sourceMesh': source['id'], 'face': face,
                                          'areaM2': exact_area,
                                          'sourceBarycentricOriginal':exact_weights.tolist(),
                                          'sourceBarycentricStored':weights.tolist(),'worldF32':world.tolist(),
                                          'reason': 'positive-source-piece-exactly-zero-world-cross-after-f32-endpoints-promoted-to-f64'})
                skipped_area += piece.area
                continue
            if abs(uv_cross)<1.e-10:
                uv_locator_repairs.append({'partId':part,'sourceMesh':source['id'],'face':face,
                                           'fragment':next_fragment,'sourceAreaM2':float(piece.area*2*AUDIT['area3'](original)),
                                           'reason':'retained-positive-world-area-UV-locator-needs-conform-and-microchart'})
            new_triangles.append({'id': {'sourceMesh': source['id'], 'primitive': source['primitive'],
                                         'sourceTriangle': face, 'fragment': next_fragment},
                                  'partId': part, 'regionId': f'{part}/reviewed-source-v25',
                                  'chartId': chart, 'world': world.tolist(),
                                  'sourceBarycentric': weights.tolist(), 'paintUv': uv.tolist()})
            made_polygons.append(poly(new_triangles[-1]))
            next_fragment += 1
            made_area += piece.area
            count += 1
        if abs(made_area+skipped_area-geometry.area) > 1e-8:
            raise ValueError('Triangulation failed to conserve the source-domain area')
        encoded=unary_union(made_polygons)
        lost=geometry.difference(encoded);gained=encoded.difference(geometry)
        if lost.area or gained.area:
            factor=2*float(AUDIT['area3'](original))
            source_domain_differences.append({'partId':part,'sourceMesh':source['id'],'face':face,
                                             'lostSourceAreaM2':lost.area*factor,'gainedSourceAreaM2':gained.area*factor,
                                             'requestedSourceGeometry':shapely.to_geojson(geometry),
                                             'lostSourceGeometry':shapely.to_geojson(lost),
                                             'gainedSourceGeometry':shapely.to_geojson(gained)})
        return count, float(made_area*2*AUDIT['area3'](original))

    left_chart = f'{LEFT}/source-uv0-v25/000'
    left_report = {'componentFaces': len(left_faces), 'exteriorVisibleFaces': len(visible_faces),
                   'reviewedFaces': len(old_left), 'backFacesExcluded': [], 'fragments': 0,
                   'coveredAreaM2': 0., 'otherPartAreaSubtractedM2': 0.}
    for face in sorted(left_faces):
        if face in visible_faces:
            target = SIMPLEX
        elif face in old_left:
            target = unary_union([selection_poly(t) for t in old_left[face]]).intersection(SIMPLEX)
        else:
            left_report['backFacesExcluded'].append(face)
            continue
        foreign = [selection_poly(t) for t in groups[(mesh['id'], face)] if t['partId'] != LEFT]
        if foreign:
            before = target.area
            target = target.difference(unary_union(foreign))
            left_report['otherPartAreaSubtractedM2'] += (before-target.area)*2*float(AUDIT['area3'](mesh['world'][face]))
        count, area = create(mesh, face, target, LEFT, left_chart)
        left_report['fragments'] += count
        left_report['coveredAreaM2'] += area

    component_owner, face_component = {}, {}
    for source in sources.values():
        if not source['paint']:
            continue
        for ordinal, faces in enumerate(AUDIT['components'](source['indices'])):
            counts = Counter(t['partId'] for face in faces for t in groups[(source['id'], face)] if t['partId'] != LEFT)
            for face in faces:
                face_component[(source['id'], face)] = ordinal
            if counts:
                owner, count = counts.most_common(1)[0]
                if count / sum(counts.values()) >= .99:
                    component_owner[(source['id'], ordinal)] = owner

    # Only known exterior meshes/components already carry reviewed part labels.
    # Completely unclassified inner panels and omitted trim stay pending.
    exterior_nodes = {13, 26, 44, 60, 78, 91, 140, 174, 198, 199}
    node175_owners = {'su7.rear-bumper', 'su7.rear-left-fender', 'su7.rear-right-fender',
                      'su7.front-left-fender', 'su7.front-right-fender', RIGHT, 'su7.charge-cover'}

    def chart_transform(chart):
        if chart in chart_fits:
            return chart_fits[chart]
        ts = [t for t in current['triangles'] if t['chartId'] == chart]
        xx, yy = [], []
        for t in ts:
            s = sources[t['id']['sourceMesh']]
            u = s['uv'][s['indices'][t['id']['sourceTriangle']]]
            xx.extend(np.asarray(t['sourceBarycentric']) @ u)
            yy.extend(t['paintUv'])
        xx = np.c_[np.asarray(xx), np.ones(len(xx))]
        yy = np.asarray(yy)
        affine = np.linalg.lstsq(xx, yy, rcond=None)[0]
        error = float(np.linalg.norm(xx@affine-yy, axis=1).max())
        chart_fits[chart] = (affine, error)
        return affine, error

    witnesses_by_face = defaultdict(list)
    for witness in visibility['witnesses']:
        witnesses_by_face[(witness['sourceMesh'], witness['face'])].append(witness)
    for key, witnesses in sorted(witnesses_by_face.items()):
        source = sources[key[0]]
        face = key[1]
        if key[0] == mesh['id'] and face in left_faces:
            continue
        owner = component_owner.get((key[0], face_component[key]))
        if source['node'] not in exterior_nodes and not (source['node'] == 175 and owner in node175_owners):
            pending.append({'sourceMesh': key[0], 'face': face, 'reason': 'inner-or-unclassified-component', 'name': source['name']})
            continue
        if not owner or source['uv'] is None:
            pending.append({'sourceMesh': key[0], 'face': face, 'reason': 'ambiguous-component-owner-or-UV0'})
            continue
        source_uv = source['uv'][source['indices'][face]].astype(float)
        uv_edges = source_uv[1:]-source_uv[0]
        if abs(float(np.linalg.det(uv_edges))) < 1e-16:
            degenerate_source_uvs.append({'sourceMesh':key[0],'face':face,'partId':owner,
                                          'reason':'new-domain-exterior-eligibility-and-UV-reunwrap-not-independently-reviewed-existing-mask-retained'})
            pending.append(degenerate_source_uvs[-1])
            continue
        normal = np.cross(source['world'][face, 1]-source['world'][face, 0], source['world'][face, 2]-source['world'][face, 0])
        normal /= max(float(np.linalg.norm(normal)), 1e-30)
        if owner == RIGHT and normal[2] >= -.2 or owner == 'su7.charge-cover' and normal[2] <= .2:
            pending.append({'sourceMesh': key[0], 'face': face, 'reason': 'unreviewed-inward-or-side-wall', 'partId': owner})
            continue
        existing = groups[key]
        diff = SIMPLEX.difference(unary_union([selection_poly(t) for t in existing]).intersection(SIMPLEX)) if existing else SIMPLEX
        if diff.is_empty:
            continue
        regions = [diff] if diff.geom_type == 'Polygon' else list(diff.geoms)
        original = source['world'][face]
        basis = (original[1:]-original[0]).T
        selected = []
        for region in regions:
            for witness in witnesses:
                weights = np.linalg.lstsq(basis, np.asarray(witness['point'])-original[0], rcond=None)[0]
                if region.distance(Point(weights)) < 1e-7:
                    selected.append(region)
                    break
        if not selected:
            continue
        candidates = [t for t in existing if t['partId'] == owner]
        if not candidates:
            candidates = [t for t in existing_parts[owner] if t['id']['sourceMesh'] == key[0] and face_component[(key[0], t['id']['sourceTriangle'])] == face_component[key]]
        if not candidates:
            pending.append({'sourceMesh': key[0], 'face': face, 'reason': 'no-reviewed-chart-in-component'})
            continue
        center = original.mean(axis=0)
        near = min(candidates, key=lambda t: float(np.sum((np.asarray(t['world']).mean(axis=0)-center)**2)))
        affine, error = chart_transform(near['chartId'])
        if error > .01:
            pending.append({'sourceMesh': key[0], 'face': face, 'reason': 'chart-is-not-a-single-affine-UV0-chart', 'maxResidualMm': error})
            continue
        count, area = create(source, face, unary_union(selected), owner, near['chartId'], affine)
        additions.append({'partId': owner, 'sourceMesh': key[0], 'face': face, 'fragments': count,
                          'restoredAreaM2': area, 'chartId': near['chartId'], 'chartFitMaxErrorMm': error})

    old_by_id = {json.dumps(t['id'], sort_keys=True): t for t in current['triangles']}
    replacements = [t for t in new_triangles if t['partId'] == LEFT]
    connections, seam_report = [], []
    # Match actual edge coordinates to the surviving partner, including edge
    # orientation. Never inherit an old edge index after replacing a triangle.
    for connection in current['connections']:
        ends = [old_by_id[json.dumps(connection[s]['triangle'], sort_keys=True)] for s in ('from', 'to')]
        is_left = [t['partId'] == LEFT for t in ends]
        if not any(is_left):
            connections.append(connection)
            continue
        if all(is_left):
            seam_report.append({'id': connection['id'], 'status': 'removed-obsolete-internal-left-link'})
            continue
        side = 'from' if is_left[0] else 'to'
        partner_side = 'to' if side == 'from' else 'from'
        partner = ends[1] if side == 'from' else ends[0]
        partner_edge = connection[partner_side]['edge']
        target = np.asarray(partner['world'])[[partner_edge, (partner_edge+1)%3]]
        best = None
        for triangle in replacements:
            for edge in range(3):
                points = np.asarray(triangle['world'])[[edge, (edge+1)%3]]
                length_error = abs(np.linalg.norm(points[1]-points[0])-np.linalg.norm(target[1]-target[0]))
                if length_error > .005:
                    continue
                for reversed_edge in (False, True):
                    compared = points[::-1] if reversed_edge else points
                    error = float(np.linalg.norm(compared-target, axis=1).max())
                    if error <= .02 and (best is None or error < best[0]):
                        best = (error, triangle, edge, reversed_edge)
        if best is None:
            seam_report.append({'id': connection['id'], 'status': 'pending-no-matching-real-edge'})
            continue
        error, triangle, edge, reversed_edge = best
        replacement = dict(connection)
        replacement[side] = {'triangle': triangle['id'], 'edge': edge}
        replacement['reversed'] = reversed_edge
        replacement['physicalGapMm'] = error*1000
        connections.append(replacement)
        seam_report.append({'id': connection['id'], 'status': 'reanchored-real-edge', 'gapMm': error*1000,
                            'edge': edge, 'reversed': reversed_edge})

    prior_retained=[]
    if prior_repair is not None:
        kept_ids={json.dumps(t['id'],sort_keys=True) for t in kept}
        # Existing non-left input triangles are already retained byte-for-byte.
        # Keep every reviewed earlier repair fragment, allowing the conform
        # union to remove duplicates without deleting its positive source area.
        for old in prior_repair['triangles']:
            if json.dumps(old['id'],sort_keys=True) in kept_ids:
                continue
            created=copy.deepcopy(old)
            created['id']['fragment']=next_fragment;next_fragment+=1
            new_triangles.append(created)
            prior_retained.append({'oldTriangle':old['id'],'retainedTriangle':created['id']})
    candidate = dict(current, paintFingerprint='', triangles=kept+new_triangles, connections=connections)
    AUDIT['write_json'](args.output/'paint-map-unsealed.json', candidate)
    summary = Counter()
    for addition in additions:
        summary[addition['partId']] += addition['restoredAreaM2']
    report = {'status': 'candidate-awaiting-seal-source-union-topology-and-desktop-review',
              'inputMap': str(args.map.resolve()), 'inputMapSha256': AUDIT['digest'](args.map),
              'modelFingerprint': model_hash, 'keptTriangles': len(kept), 'newTriangles': len(new_triangles),
              'removedLeftTriangles': len(current['triangles'])-len(kept),
              'left': left_report, 'leftChartMethod': 'actual left UV0, 90 degree rotation (u=v0,v=-u0), common right-chart millimetre scale, translation to nonnegative coordinates',
              'millimetresPerSourceUV': scale, 'rightReferenceFitMaxErrorMm': right_fit_error,
              'otherExteriorRestoredAreaM2ByPart': dict(summary), 'otherExteriorAdditions': additions,
              'pendingRegions': pending, 'precisionSlivers': precision_slivers,
              'precisionSliverAreaM2': sum(s['areaM2'] for s in precision_slivers), 'seams': seam_report,
              'retainedUVLocatorRepairPieces':uv_locator_repairs,
              'deferredDegenerateSourceUV0Faces':degenerate_source_uvs,
              'selectionBasis':args.selection_basis,
              'sourceDomainNormalization':'Full homogeneous source rows normalized before polygon union; exact dyadic determinant removes only proved source lines.',
              'requestedToStoredSourceDomainDifferences':source_domain_differences,
              'requestedToStoredLostSourceAreaM2':sum(d['lostSourceAreaM2'] for d in source_domain_differences),
              'requestedToStoredGainedSourceAreaM2':sum(d['gainedSourceAreaM2'] for d in source_domain_differences),
              'priorReviewedRepair':({'path':str(args.prior_repair.resolve()),'sha256':AUDIT['digest'](args.prior_repair),
                                      'paintFingerprint':prior_repair['paintFingerprint'],'retainedFragments':prior_retained}
                                     if prior_repair is not None else None),
              'otherTrianglesUnchanged': kept == [t for t in current['triangles'] if t['partId'] != LEFT],
              'limits': ['Visibility is sampled; hidden regions are not thereby approved or rejected.',
                         'Only reviewed outer components are extended; cowl/rear trim and inner panels remain pending.',
                         'New fragments may require T-junction adjacency handling; seal validation alone does not prove topology connectivity.',
                         'Chart replacement requires project-layer compatibility review before promotion.']}
    AUDIT['write_json'](args.output/'repair-report.json', report)
    print(json.dumps({k:report[k] for k in ('status', 'keptTriangles', 'newTriangles', 'removedLeftTriangles', 'otherExteriorRestoredAreaM2ByPart', 'otherTrianglesUnchanged')}), flush=True)


if __name__ == '__main__':
    main()
