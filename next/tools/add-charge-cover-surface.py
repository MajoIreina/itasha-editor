"""Add the verified visible left rear charging-cover faces to the SU7 paint map.

The source face list comes from a Blender BVH visibility audit of the source GLB.
The cover remains its own part and gets one side-profile chart projected onto X/Y.
"""
import hashlib
import json
import struct
import sys
from pathlib import Path

import numpy as np

TOOLS = Path(__file__).resolve().parent
sys.path.insert(0, str(TOOLS))
from paint_uv_geometry import positive_overlaps  # noqa: E402


def accessor(document, blob, index):
    spec = document["accessors"][index]
    view = document["bufferViews"][spec["bufferView"]]
    component = {
        5121: np.dtype("u1"),
        5123: np.dtype("<u2"),
        5125: np.dtype("<u4"),
        5126: np.dtype("<f4"),
    }[spec["componentType"]]
    width = {"SCALAR": 1, "VEC2": 2, "VEC3": 3, "VEC4": 4}[spec["type"]]
    offset = 20 + struct.unpack_from("<I", blob, 12)[0] + 8
    offset += view.get("byteOffset", 0) + spec.get("byteOffset", 0)
    stride = view.get("byteStride", component.itemsize * width)
    return np.ndarray(
        (spec["count"], width),
        dtype=component,
        buffer=blob,
        offset=offset,
        strides=(stride, component.itemsize),
    ).copy()


def main():
    if len(sys.argv) != 5:
        raise SystemExit(
            "usage: add-charge-cover-surface.py body.glb paint-map.json visible-faces.json output-dir"
        )
    model_path, map_path, visible_path, output_dir = map(Path, sys.argv[1:])
    output_dir.mkdir(parents=True, exist_ok=False)
    model_bytes = model_path.read_bytes()
    digest = hashlib.sha256(model_bytes).hexdigest()
    doc_len = struct.unpack_from("<I", model_bytes, 12)[0]
    gltf = json.loads(model_bytes[20 : 20 + doc_len])
    mesh_id = "body.glb/node/175/primitive/0"
    if digest != json.loads(map_path.read_text(encoding="utf-8"))["modelFingerprint"]:
        raise RuntimeError("source model fingerprint differs from the paint map")

    document = json.loads(map_path.read_text(encoding="utf-8"))
    primitive = gltf["meshes"][gltf["nodes"][175]["mesh"]]["primitives"][0]
    positions = accessor(gltf, model_bytes, primitive["attributes"]["POSITION"])
    indices = accessor(gltf, model_bytes, primitive["indices"]).reshape(-1, 3).astype(np.int64)
    visible_document = json.loads(visible_path.read_text(encoding="utf-8"))
    if isinstance(visible_document, dict):
        if visible_document.get("modelFingerprint") != digest:
            raise RuntimeError("visibility report references a different source model")
        if visible_document.get("sourceMesh") != mesh_id:
            raise RuntimeError("visibility report references a different source mesh")
        visible = visible_document.get("visibleFaces", [])
    else:
        visible = visible_document
    visible_ids = sorted({int(item["face"]) for item in visible})
    if len(visible_ids) != len(visible):
        raise RuntimeError("visibility audit contains duplicate source faces")
    if any(face < 0 or face >= len(indices) for face in visible_ids):
        raise RuntimeError("visibility audit references a face outside node 175")

    visible_world = positions[indices[visible_ids]].astype(np.float32)
    normals = np.cross(
        visible_world[:, 1] - visible_world[:, 0],
        visible_world[:, 2] - visible_world[:, 0],
    )
    normal_z = normals[:, 2] / np.maximum(np.linalg.norm(normals, axis=1), 1e-20)
    # The exterior cover face points toward the vehicle's left (+Z). Reject the
    # narrow sidewall/back faces that overlap the side-profile projection.
    keep = normal_z >= 0.2
    face_ids = [face for face, accepted in zip(visible_ids, keep) if accepted]
    worlds = positions[indices[face_ids]].astype(np.float32)
    if len(face_ids) < 400:
        raise RuntimeError(f"too few outward charging-cover faces: {len(face_ids)}")

    existing_ids = {
        (triangle["id"]["sourceMesh"], triangle["id"]["sourceTriangle"])
        for triangle in document["triangles"]
    }
    if any((mesh_id, face) in existing_ids for face in face_ids):
        raise RuntimeError("a charging-cover source face is already mapped")

    # Keep the rear cover upright in side elevation: vehicle height is horizontal,
    # and increasing longitudinal X runs from the front edge toward the rear.
    uv = np.stack((worlds[:, :, 1], worlds[:, :, 0]), axis=2) * 1000.0
    uv -= uv.reshape(-1, 2).min(axis=0)
    uv = uv.astype(np.float32)
    doubled_areas = np.abs(
        (uv[:, 1, 0] - uv[:, 0, 0]) * (uv[:, 2, 1] - uv[:, 0, 1])
        - (uv[:, 1, 1] - uv[:, 0, 1]) * (uv[:, 2, 0] - uv[:, 0, 0])
    )
    if np.any(~np.isfinite(uv)) or np.any(doubled_areas < 1e-9):
        raise RuntimeError("charging-cover projection contains a degenerate UV triangle")
    overlaps = positive_overlaps(uv.tolist(), minimum_area=1e-5)
    if overlaps:
        raise RuntimeError(f"charging-cover chart self-overlaps: {len(overlaps)} triangle pairs")

    for face, world, paint_uv in zip(face_ids, worlds, uv):
        barycentric = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]
        fragment = int.from_bytes(hashlib.sha256(world.tobytes()).digest()[:8], "little")
        document["triangles"].append(
            {
                "id": {
                    "sourceMesh": mesh_id,
                    "primitive": 0,
                    "sourceTriangle": face,
                    "fragment": fragment,
                },
                "partId": "su7.charge-cover",
                "regionId": "su7.charge-cover/exterior",
                "chartId": "su7.charge-cover/chart/0",
                "world": world.tolist(),
                "sourceBarycentric": barycentric,
                "paintUv": paint_uv.tolist(),
            }
        )
    document["paintFingerprint"] = ""
    (output_dir / "paint-map-unsealed.json").write_text(
        json.dumps(document, separators=(",", ":")), encoding="utf-8"
    )
    report = {
        "status": "candidate-requires-runtime-review",
        "modelFingerprint": digest,
        "sourceMesh": mesh_id,
        "partId": "su7.charge-cover",
        "name": "充电盖",
        "visibleSourceFaces": len(face_ids),
        "visibilityCandidates": len(visible_ids),
        "excludedSideOrBackFaces": len(visible_ids) - len(face_ids),
        "visibleSurfaceAreaM2": float(
            np.linalg.norm(
                np.cross(worlds[:, 1] - worlds[:, 0], worlds[:, 2] - worlds[:, 0]),
                axis=1,
            ).sum()
            / 2
        ),
        "chartDimensionsMm": np.ptp(uv.reshape(-1, 2), axis=0).tolist(),
        "positiveAreaOverlaps": len(overlaps),
        "sourceFaces": face_ids,
    }
    (output_dir / "charge-cover-report.json").write_text(
        json.dumps(report, ensure_ascii=False, indent=2), encoding="utf-8"
    )
    print(json.dumps({k: v for k, v in report.items() if k != "sourceFaces"}, ensure_ascii=False))


if __name__ == "__main__":
    main()
