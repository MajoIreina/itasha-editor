"""Find the visible left-rear charge-cover faces in the processed SU7 GLB.

Run with Blender 4.5.3:
  blender --background --python tools/audit-charge-cover-visibility.py -- body.glb visible-faces.json

This is an SU7-specific source audit. It identifies the disconnected paint shell
on the left rear quarter, then keeps faces visible from the assembled exterior.
"""
import hashlib
import json
import sys
from pathlib import Path

import bpy
from mathutils import Vector
from mathutils.bvhtree import BVHTree


def connected_components(mesh):
    parents = list(range(len(mesh.vertices)))

    def root(index):
        while parents[index] != index:
            parents[index] = parents[parents[index]]
            index = parents[index]
        return index

    for polygon in mesh.polygons:
        vertices = polygon.vertices
        for vertex in vertices[1:]:
            left, right = root(vertices[0]), root(vertex)
            if left != right:
                parents[right] = left
    groups = {}
    for polygon in mesh.polygons:
        groups.setdefault(root(polygon.vertices[0]), []).append(polygon)
    return list(groups.values())


def component_bounds(obj, polygons):
    points = [obj.matrix_world @ obj.data.vertices[index].co for p in polygons for index in p.vertices]
    low = Vector((min(p.x for p in points), min(p.y for p in points), min(p.z for p in points)))
    high = Vector((max(p.x for p in points), max(p.y for p in points), max(p.z for p in points)))
    return low, high


def main():
    args = sys.argv[sys.argv.index("--") + 1 :]
    if len(args) != 2:
        raise SystemExit("usage: audit-charge-cover-visibility.py body.glb visible-faces.json")
    model_path, output_path = map(Path, args)
    model_path = model_path.resolve()
    output_path = output_path.resolve()
    bpy.ops.import_scene.gltf(filepath=str(model_path))
    obj = next(
        (item for item in bpy.data.objects if item.type == "MESH" and item.name == "carPaint_internal_1"),
        None,
    )
    if obj is None:
        raise RuntimeError("SU7 left-rear paint shell carPaint_internal_1 is missing")

    # Blender imports the glTF axes as X=longitudinal, Y=-lateral, Z=up.
    # The cover is the small disconnected plate around (1.835, -0.851, 0.918).
    candidates = []
    for polygons in connected_components(obj.data):
        low, high = component_bounds(obj, polygons)
        center = (low + high) * 0.5
        if (
            300 <= len(polygons) <= 1000
            and 1.60 < center.x < 2.05
            and -1.00 < center.y < -0.70
            and 0.75 < center.z < 1.02
        ):
            distance = (center - Vector((1.835, -0.851, 0.918))).length
            candidates.append((distance, polygons, low, high))
    if len(candidates) != 1:
        raise RuntimeError(f"expected one left-rear charging-cover component; found {len(candidates)}")
    _, target, low, high = candidates[0]
    if (high.x - low.x) > 0.40 or (high.z - low.z) > 0.30:
        raise RuntimeError("candidate cover dimensions fall outside the SU7 baseline range")

    vertices, faces = [], []
    for item in bpy.data.objects:
        if item.type != "MESH":
            continue
        offset = len(vertices)
        transform = item.matrix_world
        vertices.extend(transform @ vertex.co for vertex in item.data.vertices)
        faces.extend(
            tuple(offset + index for index in polygon.vertices)
            for polygon in item.data.polygons
            if len(polygon.vertices) == 3
        )
    tree = BVHTree.FromPolygons(vertices, faces, epsilon=0, all_triangles=True)

    visible = []
    for polygon in target:
        points = [obj.matrix_world @ obj.data.vertices[index].co for index in polygon.vertices]
        normal = (points[1] - points[0]).cross(points[2] - points[0])
        area = normal.length * 0.5
        if area < 1e-12:
            continue
        normal.normalize()
        center = sum(points, Vector()) / 3
        cameras = [
            Vector((center.x, -7, center.z)),
            Vector((center.x - 3, -7, center.z)),
            Vector((center.x + 3, -7, center.z)),
            Vector((center.x, -5, center.z + 2)),
            Vector((center.x, -5, center.z - 2)),
            Vector((center.x - 2, -5, center.z + 1)),
            Vector((center.x + 2, -5, center.z + 1)),
            Vector((center.x, -8, 1.8)),
        ]
        for camera_index, camera in enumerate(cameras):
            direction = center - camera
            distance = direction.length
            if normal.dot(-direction.normalized()) <= 0.02:
                continue
            hit, _, _, _ = tree.ray_cast(camera, direction.normalized(), distance + 0.002)
            if hit is not None and (hit - center).length < 0.001:
                visible.append(
                    {
                        "face": polygon.index,
                        "area": area,
                        "center": list(center),
                        "normal": list(normal),
                        "camera": camera_index,
                    }
                )
                break

    if len(visible) < 500:
        raise RuntimeError(f"too few exterior-visible cover faces: {len(visible)}")
    output_path.parent.mkdir(parents=True, exist_ok=True)
    output_path.write_text(
        json.dumps(
            {
                "modelFingerprint": hashlib.sha256(model_path.read_bytes()).hexdigest(),
                "sourceMesh": "body.glb/node/175/primitive/0",
                "visibleFaces": visible,
            },
            separators=(",", ":"),
        ),
        encoding="utf-8",
    )
    print(json.dumps({"output": str(output_path), "candidateFaces": len(target), "visibleFaces": len(visible)}))


if __name__ == "__main__":
    main()
