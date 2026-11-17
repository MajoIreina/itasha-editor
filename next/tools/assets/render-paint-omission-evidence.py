"""Render diagnostic, occluded whole-car and close views of missing paint.

Run with Blender --background --factory-startup --python THIS -- --model BODY
--map MAP --audit AUDIT_DIRECTORY --output NEW_DIRECTORY. Orange is the source
paint face; covered fragments are painted gray above it. No hidden-object view
is used. Rendered colors are diagnostic labels, not OEM material appearance.
"""
from __future__ import annotations
import argparse
from collections import defaultdict
import json
from pathlib import Path
import runpy
import sys

import bpy
import numpy as np
from mathutils import Vector

AUDIT = runpy.run_path(str(Path(__file__).with_name('audit-paint-integrity.py')))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--model', type=Path, required=True)
    parser.add_argument('--map', type=Path, required=True)
    parser.add_argument('--audit', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--regions', nargs='*')
    parser.add_argument('--highlight-face',nargs=2,type=int,metavar=('NODE','FACE'),help='Highlight one complete source face for part semantics, including covered paint')
    parser.add_argument('--audit-face',nargs=2,type=int,metavar=('NODE','FACE'),help='Render one source face with actual sampled visibility witnesses and mapped overlays')
    parser.add_argument('--detail-scale',type=float,default=.34,help='Detail orthographic width in metres; reduce for microscopic source faces')
    parser.add_argument('--context-scale',type=float,help='Optional context orthographic width in metres')
    args = parser.parse_args(sys.argv[sys.argv.index('--')+1:])
    if args.detail_scale <= 0 or (args.context_scale is not None and args.context_scale <= 0):
        parser.error('Orthographic widths must be positive')
    if args.highlight_face and args.audit_face:
        parser.error('--highlight-face and --audit-face are mutually exclusive')
    args.model=args.model.resolve();args.map=args.map.resolve();args.audit=args.audit.resolve();args.output=args.output.resolve()
    args.output.mkdir(parents=True, exist_ok=False)
    paint_map = json.loads(args.map.read_text('utf-8'))
    visibility = json.loads((args.audit/'visibility.json').read_text('utf-8'))
    missing = json.loads((args.audit/'uncovered-source-faces.json').read_text('utf-8'))
    config = json.loads((args.model.parent/'config.json').read_text('utf-8'))
    modules = [args.model, args.model.parent/config['wheel'][config.get('defaults',{}).get('wheel',0)]['asset']]
    primitives = [p for module in modules for p in AUDIT['Glb'](module).primitives()]
    sources = {p['id']:p for p in primitives}
    body175 = next(p for p in primitives if p['id'].startswith('body.glb/') and p['node']==175)
    components = AUDIT['components'](body175['indices'])
    groups = {'hood-inner':set(), 'hood-outer':set(), 'trunk-inner':set(),
              'front-left-door-fold':set(),'front-right-door-fold':set(),
              'rear-left-door-fold':set(),'rear-right-door-fold':set(),
              'cowl':set(), 'rear-trim':set(),
              'left-sill':{(body175['id'],f) for f in components[5]},
              'right-sill':{(body175['id'],f) for f in components[6]},
              'charge-sidewall':set()}
    for p in primitives:
        if not p['id'].startswith('body.glb/'):
            continue
        name = {13:'hood-outer',14:'hood-inner',16:'cowl',92:'trunk-inner',
                44:'front-left-door-fold',78:'front-right-door-fold',
                26:'rear-left-door-fold',60:'rear-right-door-fold',180:'rear-trim'}.get(p['node'])
        if name:
            groups[name] = {(p['id'],f) for f in range(len(p['world']))}
    for face in components[9]:
        w = body175['world'][face]
        normal = np.cross(w[1]-w[0],w[2]-w[0]); normal /= np.linalg.norm(normal)
        if normal[2] <= .2:
            groups['charge-sidewall'].add((body175['id'],face))
    if args.regions:
        groups={name:faces for name,faces in groups.items() if name in args.regions}
    if args.highlight_face or args.audit_face:
        node,face=args.highlight_face or args.audit_face
        source=next(p for p in primitives if p['id'].startswith('body.glb/') and p['node']==node)
        prefix='part-correction' if args.highlight_face else 'source-omission'
        groups={f'{prefix}-{node}-{face}':{(source['id'],face)}}
    bpy.ops.object.select_all(action='SELECT'); bpy.ops.object.delete(use_global=False)
    gray = bpy.data.materials.new('Diagnostic body');gray.diffuse_color=(.42,.48,.55,1.)
    dark = bpy.data.materials.new('Diagnostic nonpaint');dark.diffuse_color=(.11,.14,.18,1.)
    orange = bpy.data.materials.new('Uncovered source paint');orange.diffuse_color=(1.,.15,.015,1.)
    meshes = {}
    for p in primitives:
        mesh = bpy.data.meshes.new(p['id']); mesh.from_pydata(p['positions'].tolist(),[],p['indices'].tolist());mesh.update()
        obj = bpy.data.objects.new(p['id'],mesh);bpy.context.collection.objects.link(obj)
        mesh.materials.append(gray if p['paint'] else dark);mesh.materials.append(orange)
        meshes[p['id']] = mesh
    # The map is drawn only over highlighted source faces. This leaves the true
    # complement orange, including partial-face holes, with model occlusion intact.
    mapped = defaultdict(list)
    for t in paint_map['triangles']:
        mapped[(t['id']['sourceMesh'],t['id']['sourceTriangle'])].append(t)
    scene = bpy.context.scene;scene.render.engine='BLENDER_WORKBENCH'
    scene.display.shading.light='STUDIO';scene.display.shading.studiolight_rotate_z=.4
    scene.display.shading.color_type='MATERIAL';scene.display.shading.show_shadows=True
    scene.display.shading.show_cavity=True;scene.display.shading.cavity_type='BOTH'
    scene.display.shading.show_specular_highlight=True
    scene.display.shading.background_type='WORLD';scene.world.color=(.055,.065,.085)
    scene.view_settings.view_transform='Standard'
    scene.render.resolution_x=1800;scene.render.resolution_y=1200;scene.render.resolution_percentage=100
    scene.render.image_settings.file_format='PNG';scene.render.film_transparent=False
    camera_data=bpy.data.cameras.new('Evidence camera');camera=bpy.data.objects.new('Evidence camera',camera_data)
    bpy.context.collection.objects.link(camera);scene.camera=camera
    camera_data.type='ORTHO';camera_data.clip_start=.001;camera_data.clip_end=100
    evidence=[]
    for name, selected in groups.items():
        selected_missing=[f for f in missing if (f['sourceMesh'],f['face']) in selected]
        witnesses=[w for w in visibility['witnesses'] if (w['sourceMesh'],w['face']) in selected]
        for source_id,face in selected:
            meshes[source_id].polygons[face].material_index=1
        overlay_vertices=[];overlay_faces=[]
        for face_key in selected:
            source=sources[face_key[0]];source_world=source['world'][face_key[1]]
            normal=np.cross(source_world[1]-source_world[0],source_world[2]-source_world[0]);normal/=max(np.linalg.norm(normal),1.e-30)
            for triangle in ([] if args.highlight_face else mapped[face_key]):
                start=len(overlay_vertices)
                overlay_vertices.extend((np.asarray(triangle['world'])+normal*.000035).tolist())
                overlay_faces.append((start,start+1,start+2))
        mesh=bpy.data.meshes.new('Covered overlay');mesh.from_pydata(overlay_vertices,[],overlay_faces);mesh.materials.append(gray)
        overlay=bpy.data.objects.new('Covered overlay',mesh);bpy.context.collection.objects.link(overlay)
        points=np.concatenate([sources[s]['world'][f] for s,f in selected])
        center=points.mean(axis=0)
        views=[]
        if args.highlight_face:
            normal=np.cross(points[1]-points[0],points[2]-points[0]);normal/=np.linalg.norm(normal)
            witnesses=[{'sourceMesh':next(iter(selected))[0],'face':args.highlight_face[1],
                        'camera':(center+normal*7).tolist(),'point':center.tolist(),'uncoveredRegionAreaM2':0.,
                        'semanticSourceHighlightOnly':True,'rayVisibilityProved':False}]
        if witnesses:
            chosen=sorted(witnesses,key=lambda w:w['uncoveredRegionAreaM2'],reverse=True)
            chosen=[chosen[0]]+[w for w in chosen[1:] if np.linalg.norm(np.asarray(w['point'])-chosen[0]['point'])>.08][:1]
            for ordinal,witness in enumerate(chosen):
                eye=np.asarray(witness['camera']);target=np.asarray(witness['point'])
                for detail in (False,True):
                    aim=target if detail else (np.array([0.,.7,0.]) if args.highlight_face else center)
                    location=aim+(eye-target)/np.linalg.norm(eye-target)*7
                    camera.location=Vector(location);camera.rotation_euler=(Vector(aim)-camera.location).to_track_quat('-Z','Y').to_euler()
                    camera_data.ortho_scale=(args.detail_scale if detail else
                                             (args.context_scale if args.context_scale is not None else
                                              (6.2 if args.highlight_face else max(.65,float(np.ptp(points,axis=0).max())*1.25))))
                    filename=f'{name}-{ordinal+1}-'+('detail' if detail else 'context')+'.png'
                    scene.render.filepath=str(args.output/filename);bpy.ops.render.render(write_still=True)
                    views.append({'file':filename,'camera':location.tolist(),'target':aim.tolist(),
                                  'orthographicWidthM':camera_data.ortho_scale,'witness':witness})
        else:
            location=center+np.array([-.3,1.2,2.])
            camera.location=Vector(location);camera.rotation_euler=(Vector(center)-camera.location).to_track_quat('-Z','Y').to_euler()
            camera_data.ortho_scale=max(.65,float(np.ptp(points,axis=0).max())*1.25)
            filename=f'{name}-no-exterior-ray-witness.png';scene.render.filepath=str(args.output/filename)
            bpy.ops.render.render(write_still=True);views.append({'file':filename,'witness':None})
        evidence.append({'region':name,'sourcePaintFaces':len(selected),
                         'sourceMaterials':list({sources[s]['material'] for s,f in selected}),
                         'allNativePaintSwitch':all(sources[s]['paint'] for s,f in selected),
                         'uncoveredAreaM2':sum(f['uncoveredAreaM2'] for f in selected_missing),
                         'visibleWitnessFaces':len({w['face'] for w in witnesses if w.get('rayVisibilityProved',True)}),
                         'sampledVisiblePolygonAreaUpperBoundM2':sum(w['uncoveredRegionAreaM2'] for w in witnesses),
                         'sourceFaces':[{'sourceMesh':s,'face':f} for s,f in sorted(selected)],'views':views})
        bpy.data.objects.remove(overlay,do_unlink=True)
        for source_id,face in selected:
            meshes[source_id].polygons[face].material_index=0
    report={'modelFingerprint':AUDIT['digest'](args.model),'mapSha256':AUDIT['digest'](args.map),
            'mode':'source-face-highlight' if args.highlight_face else 'uncovered-region-evidence',
            'diagnosticColors':('orange=entire selected source face including covered paint, gray=body, dark=other source materials'
                                if args.highlight_face else
                                'orange=uncovered native-paint-switch source faces, gray=mapped paint/body, dark=other source materials'),
            'occlusion':'all body and default wheel geometry retained, no hidden-object inspection',
            'limits':['Workbench diagnostic colors do not reproduce source texture/opacity.',
                      'Ray witnesses prove a sampled open-model sight line; source native_paint_switch is separate material evidence.',
                      'Observed polygon area is an upper bound, not exact visible subarea.',
                      'Source-face highlights do not prove an unobstructed sight line or semantic exterior eligibility.'], 'regions':evidence}
    AUDIT['write_json'](args.output/'evidence.json',report)
    print('EVIDENCE_RENDER_COMPLETE',flush=True)


if __name__=='__main__':main()
