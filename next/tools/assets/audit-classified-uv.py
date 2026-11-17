"""Read-only audit of classified AO masks against v16 source UV0 geometry.

The script changes no model or mapping asset. It writes one JSON report. Defaults
refer to this SU7 workspace; pass paths explicitly when auditing another input.

python tools/assets/audit-classified-uv.py --output .reports/classified-uv-20261111.json
"""
from __future__ import annotations

import argparse
from collections import Counter, defaultdict
import hashlib
import json
import math
from pathlib import Path
import re
import struct
import time

import numpy as np
from PIL import Image, ImageDraw

from paint_uv_geometry import positive_overlaps
from classified_paint_masks import DEFAULT_AO, DEFAULT_CSV, load_classified_masks


NEXT = Path(__file__).resolve().parents[2]
WORKSPACE = NEXT.parents[1]
def sha256(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def area2(triangles):
    a, b = triangles[:, 1] - triangles[:, 0], triangles[:, 2] - triangles[:, 0]
    return np.abs(a[:, 0] * b[:, 1] - a[:, 1] * b[:, 0]) * .5


def area3(triangles):
    return np.linalg.norm(np.cross(triangles[:, 1] - triangles[:, 0],
                                   triangles[:, 2] - triangles[:, 0]), axis=1) * .5


def quantiles(values):
    values = np.asarray(values)
    values = values[np.isfinite(values)]
    return {"p" + str(p): float(np.percentile(values, p)) for p in (0, 5, 50, 95, 99, 100)} if len(values) else {}


def triangle_samples(n=4):
    """Centroids of n**2 equal-area subdivisions, deterministic and interior."""
    points = []
    for i in range(n):
        for j in range(n - i):
            x, y = (i + 1 / 3) / n, (j + 1 / 3) / n
            points.append((1 - x - y, x, y))
            if i + j < n - 1:
                x, y = (i + 2 / 3) / n, (j + 2 / 3) / n
                points.append((1 - x - y, x, y))
    return np.asarray(points)


class Glb:
    def __init__(self, path):
        self.path = path
        self.raw = path.read_bytes()
        magic, version, length = struct.unpack_from("<III", self.raw)
        if magic != 0x46546C67 or version != 2 or length != len(self.raw):
            raise ValueError("invalid GLB header")
        self.document = None
        self.binary = None
        offset = 12
        while offset < len(self.raw):
            size, kind = struct.unpack_from("<II", self.raw, offset)
            data = self.raw[offset + 8:offset + 8 + size]
            if kind == 0x4E4F534A:
                self.document = json.loads(data)
            elif kind == 0x004E4942:
                self.binary = data
            offset += 8 + size
        if self.document is None or self.binary is None:
            raise ValueError("GLB must have embedded JSON and BIN chunks")
        self.transforms = {}
        self.cache = {}
        doc = self.document
        for index in doc["scenes"][doc.get("scene", 0)]["nodes"]:
            self.visit(index, np.eye(4))

    def visit(self, index, parent):
        node = self.document["nodes"][index]
        if "matrix" in node:
            local = np.asarray(node["matrix"]).reshape(4, 4).T
        else:
            x, y, z, w = node.get("rotation", [0., 0., 0., 1.])
            local = np.eye(4)
            local[:3, :3] = [[1-2*(y*y+z*z), 2*(x*y-z*w), 2*(x*z+y*w)],
                             [2*(x*y+z*w), 1-2*(x*x+z*z), 2*(y*z-x*w)],
                             [2*(x*z-y*w), 2*(y*z+x*w), 1-2*(x*x+y*y)]]
            local[:3, :3] *= np.asarray(node.get("scale", [1., 1., 1.]))
            local[:3, 3] = node.get("translation", [0., 0., 0.])
        transform = parent @ local
        self.transforms[index] = transform
        for child in node.get("children", []):
            self.visit(child, transform)

    def accessor(self, index):
        spec = self.document["accessors"][index]
        if "sparse" in spec:
            raise ValueError("sparse accessors need explicit implementation")
        view = self.document["bufferViews"][spec["bufferView"]]
        if view.get("buffer", 0) != 0:
            raise ValueError("only embedded buffer zero is supported")
        dtype = np.dtype({5120: "i1", 5121: "u1", 5122: "<i2", 5123: "<u2",
                          5125: "<u4", 5126: "<f4"}[spec["componentType"]])
        width = {"SCALAR": 1, "VEC2": 2, "VEC3": 3, "VEC4": 4}[spec["type"]]
        values = np.ndarray((spec["count"], width), dtype=dtype, buffer=self.binary,
                            offset=view.get("byteOffset", 0) + spec.get("byteOffset", 0),
                            strides=(view.get("byteStride", dtype.itemsize * width), dtype.itemsize)).copy()
        if spec.get("normalized", False) and dtype.kind in "ui":
            info = np.iinfo(dtype)
            values = values.astype(np.float64) / info.max
            if dtype.kind == "i":
                values = np.maximum(values, -1)
        return values

    def geometry(self, mesh_id):
        if mesh_id in self.cache:
            return self.cache[mesh_id]
        match = re.fullmatch(re.escape(self.path.name) + r"/node/(\d+)/primitive/(\d+)", mesh_id)
        if match is None:
            raise ValueError(f"unexpected source mesh: {mesh_id}")
        node_id, primitive_id = map(int, match.groups())
        node = self.document["nodes"][node_id]
        primitive = self.document["meshes"][node["mesh"]]["primitives"][primitive_id]
        if primitive.get("mode", 4) != 4:
            raise ValueError("only triangular primitives are supported")
        positions = self.accessor(primitive["attributes"]["POSITION"]).astype(np.float64)
        uv = self.accessor(primitive["attributes"]["TEXCOORD_0"]).astype(np.float64)
        indices = self.accessor(primitive["indices"]).reshape(-1, 3).astype(np.int64) if "indices" in primitive else np.arange(len(positions)).reshape(-1, 3)
        transform = self.transforms[node_id]
        mirrored = np.linalg.det(transform) < 0
        # Must match renderer::assets::Loader::node before applying stored barycentrics.
        if mirrored:
            indices = indices[:, [0, 2, 1]]
        world = positions @ transform[:3, :3].T + transform[:3, 3]
        geometry = {"world": world[indices], "uv": uv[indices], "mirrored": mirrored,
                    "nodeName": node.get("name", ""), "triangles": len(indices)}
        self.cache[mesh_id] = geometry
        return geometry


def overlap_summary(uv, ids, minimum_area):
    overlaps = positive_overlaps(uv.tolist(), minimum_area=minimum_area)
    return {"minimumAreaPx2": minimum_area, "positiveAreaPairCount": len(overlaps),
            "pairCountAbovePoint01Px2": sum(area > .01 for _, _, area in overlaps),
            "pairCountAboveOnePx2": sum(area > 1. for _, _, area in overlaps),
            "pairwiseAreaSumPx2": sum(area for _, _, area in overlaps),
            "note": "Pairwise sum is not union overlap area; shared zero-area edges are legal.",
            "samples": [{"a": ids[a], "b": ids[b], "areaPx2": area}
                        for a, b, area in sorted(overlaps, key=lambda item: -item[2])[:12]]}


def distortion(world, uv, scale):
    e, f = world[:, 1] - world[:, 0], world[:, 2] - world[:, 0]
    length = np.linalg.norm(e, axis=1)
    height = np.linalg.norm(np.cross(e, f), axis=1) / np.maximum(length, 1e-30)
    valid = (length > 1e-12) & (height > 1e-12)
    x = np.sum(e[valid] * f[valid], axis=1) / length[valid]
    intrinsic = np.zeros((len(x), 2, 2))
    intrinsic[:, 0, 0], intrinsic[:, 0, 1], intrinsic[:, 1, 1] = length[valid], x, height[valid]
    delta_uv = np.stack((uv[valid, 1] - uv[valid, 0], uv[valid, 2] - uv[valid, 0]), axis=2)
    singular = np.linalg.svd((delta_uv * scale / 1000) @ np.linalg.inv(intrinsic), compute_uv=False)
    anisotropy = singular[:, 0] / np.maximum(singular[:, 1], 1e-30)
    return {"validWorldTriangles": int(valid.sum()), "anisotropy": quantiles(anisotropy),
            "localLengthScale": quantiles(singular.reshape(-1))}


def raster_mask(uv, size):
    image = Image.new("1", size)
    draw = ImageDraw.Draw(image)
    for triangle in uv:
        draw.polygon([tuple(np.rint(p).astype(int)) for p in triangle], fill=1)
    return np.asarray(image, dtype=bool)


def sample_mask_fraction(mask, uv, size):
    uv_samples = np.einsum("sc,tcd->tsd", triangle_samples(), uv)
    coords = np.rint(uv_samples).astype(np.int64)
    inside = (coords[:, :, 0] >= 0) & (coords[:, :, 0] < size[0]) & (coords[:, :, 1] >= 0) & (coords[:, :, 1] < size[1])
    accepted = np.zeros(inside.shape, dtype=bool)
    accepted[inside] = mask[coords[:, :, 1][inside], coords[:, :, 0][inside]]
    return accepted.mean(axis=1)


def raster_comparison(mask, uv, world_areas, size):
    actual = raster_mask(uv, size)
    inter = int(np.count_nonzero(actual & mask))
    actual_count, classified_count = int(actual.sum()), int(mask.sum())
    fractions = sample_mask_fraction(mask, uv, size)
    sample_count = int(triangle_samples().shape[0])
    filter_analysis = {}
    for numerator in (8, 12, 14, 15, 16):
        threshold = numerator / sample_count
        kept = fractions >= threshold
        kept_world_area = float(world_areas[kept].sum())
        inside_world_area = float(np.dot(fractions[kept], world_areas[kept]))
        filter_analysis[f"atLeast{numerator}of{sample_count}Samples"] = {
            "threshold": threshold,
            "keptTriangles": int(kept.sum()),
            "keptTriangleFraction": float(kept.mean()) if len(kept) else 0.,
            "keptWorldAreaM2": kept_world_area,
            "keptWorldAreaFraction": kept_world_area / max(float(world_areas.sum()), 1e-30),
            "keptWorldAreaInsideMaskM2": inside_world_area,
            "keptWorldAreaOutsideMaskM2": kept_world_area - inside_world_area,
            "keptWorldAreaPrecision": inside_world_area / max(kept_world_area, 1e-30),
        }
    return {
        "geometryRasterPixels": actual_count,
        "classifiedAlphaPixels": classified_count,
        "intersectionPixels": inter,
        "geometryPrecisionInsideMask": inter / max(actual_count, 1),
        "maskRecallCoveredByGeometry": inter / max(classified_count, 1),
        "rasterIoU": inter / max(actual_count + classified_count - inter, 1),
        "worldAreaSampledInsideMaskM2": float(np.dot(fractions, world_areas)),
        "worldAreaSampledOutsideMaskM2": float(np.dot(1 - fractions, world_areas)),
        "worldAreaSampledInsideFraction": float(np.dot(fractions, world_areas) / max(world_areas.sum(), 1e-30)),
        "trianglesAllSamplesInside": int(np.count_nonzero(fractions == 1)),
        "trianglesAllSamplesOutside": int(np.count_nonzero(fractions == 0)),
        "trianglesStraddlingMask": int(np.count_nonzero((fractions > 0) & (fractions < 1))),
        "interiorSampleCountPerTriangle": sample_count,
        "sampleFractionHistogram": {f"{i}/{sample_count}": int(np.count_nonzero(fractions == i / sample_count))
                                     for i in range(sample_count + 1)},
        "wholeTriangleFilterCandidates": filter_analysis,
    }


def classification_audit(mask, uv, world_areas, size, source_geometries):
    mapped = raster_comparison(mask, uv, world_areas, size)
    source_uv, source_world_areas = [], []
    source_mesh_area = 0.
    classified_source_area = 0.
    source_triangle_count = 0
    classified_source_triangle_count = 0
    for geometry in source_geometries.values():
        source_triangles_uv = geometry["uv"].copy()
        source_triangles_uv[:, :, 0] *= size[0]
        source_triangles_uv[:, :, 1] *= size[1]
        source_areas = area3(geometry["world"])
        source_fraction = sample_mask_fraction(mask, source_triangles_uv, size)
        source_uv.append(source_triangles_uv)
        source_world_areas.append(source_areas)
        source_mesh_area += float(source_areas.sum())
        classified_source_area += float(np.dot(source_fraction, source_areas))
        source_triangle_count += len(source_areas)
        classified_source_triangle_count += int(np.count_nonzero(source_fraction > 0))
    full_source = raster_comparison(mask, np.concatenate(source_uv), np.concatenate(source_world_areas), size)
    return {"method": "round-to-pixel UV raster plus 16 equal-world-area interior samples; full-source primitive and editor-map domains reported separately",
            "mapFragmentGeometry": mapped,
            "fullReferencedSourcePrimitives": full_source,
            "mappedRasterPixels": mapped["geometryRasterPixels"],
            "classifiedAlphaPixels": mapped["classifiedAlphaPixels"],
            "intersectionPixels": mapped["intersectionPixels"],
            "mappedRasterInsideClassification": mapped["geometryPrecisionInsideMask"],
            "classifiedRasterCoveredByMap": mapped["maskRecallCoveredByGeometry"],
            "rasterIoU": mapped["rasterIoU"],
            "worldAreaSampledInsideClassificationM2": mapped["worldAreaSampledInsideMaskM2"],
            "worldAreaSampledOutsideClassificationM2": mapped["worldAreaSampledOutsideMaskM2"],
            "worldAreaSampledInsideFraction": mapped["worldAreaSampledInsideFraction"],
            "classificationSourceMeshTriangleCount": source_triangle_count,
            "classificationSourceMeshWorldAreaM2": source_mesh_area,
            "classificationSourceMeshAreaCoveredByMaskM2": classified_source_area,
            "classificationSourceTrianglesWithAnyMaskSample": classified_source_triangle_count,
            "classificationSourceMeshAreaCoveredFraction": classified_source_area / max(source_mesh_area, 1e-30),
            "mappedExteriorCoverageOfClassifiedSourceArea": mapped["worldAreaSampledInsideMaskM2"] / max(classified_source_area, 1e-30),
            "fragmentsAllSamplesInside": mapped["trianglesAllSamplesInside"],
            "fragmentsAllSamplesOutside": mapped["trianglesAllSamplesOutside"],
            "fragmentsStraddlingClassification": mapped["trianglesStraddlingMask"]}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--model", type=Path, default=WORKSPACE / "3D/Blender/software/body.glb")
    parser.add_argument("--map", dest="mapping", type=Path, default=NEXT / "assets/paint/su7-v16-classified-unfolding/paint-map.json")
    parser.add_argument("--classification", type=Path, default=WORKSPACE / "3D/车身AO_分件/按模型部件/已分类")
    parser.add_argument("--manifest", type=Path, default=WORKSPACE / "3D/车身AO_分件/manifest.json")
    parser.add_argument("--coordinate-csv", type=Path, default=DEFAULT_CSV)
    parser.add_argument("--ao", type=Path, default=DEFAULT_AO)
    parser.add_argument("--output", type=Path, default=NEXT / ".reports/classified-uv-20261111.json")
    parser.add_argument("--minimum-overlap-area", type=float, default=.0001)
    options = parser.parse_args()
    started = time.monotonic()
    model = Glb(options.model)
    document = json.loads(options.mapping.read_text(encoding="utf-8"))
    model_hash = sha256(options.model)
    if document["modelFingerprint"] != model_hash:
        raise ValueError("paint map does not reference this source GLB fingerprint")
    manifest = json.loads(options.manifest.read_text(encoding="utf-8"))
    size = tuple(manifest["source_size"])
    masks, mask_registration = load_classified_masks(options.classification, options.coordinate_csv, options.ao)
    if tuple(reversed(next(iter(masks.values())).shape)) != size:
        raise ValueError("classified masks and map atlas dimensions disagree")
    mask_sources = {entry["partId"]: entry for entry in mask_registration["sources"]}
    by_id = {part["part_id"]: part for part in manifest["parts"]}
    groups = defaultdict(list)
    for triangle in document["triangles"]:
        groups[triangle["partId"]].append(triangle)
    report = {"status": "source-uv-and-classified-mask-audit-not-semantic-approval", "inputs": {
        "model": str(options.model.resolve()), "modelSha256": model_hash,
        "map": str(options.mapping.resolve()), "mapSha256": sha256(options.mapping),
        "classificationDirectory": str(options.classification.resolve()),
        "manifest": str(options.manifest.resolve()), "manifestSha256": sha256(options.manifest)},
        "coordinateConvention": {"atlasSizePx": size, "uv0ToPixel": "x=u*W, y=v*H; body.glb UV0 is vertically flipped relative to Blender AO masks; PNG origin top-left",
            "mirroredNode": "swap source index corners 1 and 2 when baked node determinant<0, then apply sourceBarycentric",
            "runtimeNumbers": "source UV, map barycentric and map world reconstructed as f32 before f64 audit calculations",
            "overlapThresholdPx2": options.minimum_overlap_area},
        "classificationRegistration": mask_registration,
        "limitations": ["Full referenced source primitives may contain hidden/backside UV; map-fragment comparisons describe the current editor paint domain.",
            "Raster alpha and 16-point interior sampling approximate boundary membership; they do not provide exact polygon clipping.",
            "A uniform area scale within each part preserves source UV anisotropy; area agreement alone does not certify fabrication dimensions."],
        "parts": []}
    total_world_area, total_uv_area = 0., 0.
    for part_id, triangles in groups.items():
        world = np.asarray([t["world"] for t in triangles], dtype=np.float32).astype(np.float64)
        bary = np.asarray([t["sourceBarycentric"] for t in triangles], dtype=np.float32).astype(np.float64)
        source_world, source_uv, mirrored, unique_sources, source_mesh_geometries = [], [], [], {}, {}
        for triangle in triangles:
            identity = triangle["id"]
            source = model.geometry(identity["sourceMesh"])
            source_mesh_geometries[identity["sourceMesh"]] = source
            face = identity["sourceTriangle"]
            if not 0 <= face < source["triangles"]:
                raise ValueError("map source face is outside GLB primitive")
            source_world.append(source["world"][face])
            source_uv.append(source["uv"][face])
            mirrored.append(source["mirrored"])
            unique_sources[(identity["sourceMesh"], face)] = (source["world"][face], source["uv"][face])
        original_world, original_uv = np.asarray(source_world), np.asarray(source_uv)
        reconstructed_world = bary @ original_world
        uv = bary @ original_uv
        uv[:, :, 0] *= size[0]
        uv[:, :, 1] *= size[1]
        uv_area = area2(uv)
        world_area = area3(world)
        unique_world = np.asarray([v[0] for v in unique_sources.values()])
        unique_uv = np.asarray([v[1] for v in unique_sources.values()])
        unique_uv[:, :, 0] *= size[0]
        unique_uv[:, :, 1] *= size[1]
        world_sum, uv_sum = float(world_area.sum()), float(uv_area.sum())
        scale = math.sqrt(world_sum * 1e6 / uv_sum) if uv_sum > 0 else 0.
        total_world_area += world_sum
        total_uv_area += uv_sum
        uv_min, uv_max = uv.reshape(-1, 2).min(axis=0), uv.reshape(-1, 2).max(axis=0)
        errors = np.linalg.norm(reconstructed_world - world, axis=2)
        record = {"partId": part_id, "fragments": len(triangles), "referencedSourceFaces": len(unique_sources),
            "sourceMeshes": dict(Counter(t["id"]["sourceMesh"] for t in triangles)),
            "mirroredNodeFragments": int(sum(mirrored)), "worldReconstructionErrorM": quantiles(errors.reshape(-1)),
            "verticesWithWorldReconstructionErrorAbove5um": int(np.count_nonzero(errors > 5e-6)),
            "barycentricSumErrorMax": float(np.max(np.abs(bary.sum(axis=2) - 1))),
            "worldSurfaceAreaM2": world_sum, "sourceUvAreaPx2": uv_sum,
            "referencedFullSourceFaceAreaM2": float(area3(unique_world).sum()),
            "existingExteriorFragmentAreaFractionOfReferencedSourceFaces": world_sum / max(float(area3(unique_world).sum()), 1e-30),
            "sourceUvAtlasBoundsPx": [uv_min.tolist(), uv_max.tolist()],
            "degenerateSourceUvFragmentsAt1eMinus8Px2": int(np.count_nonzero(uv_area <= 1e-8)),
            "nearDegenerateSourceUvFragmentsAt1eMinus4Px2": int(np.count_nonzero(uv_area <= 1e-4)),
            "degenerateWorldFragmentsAt1eMinus12M2": int(np.count_nonzero(world_area <= 1e-12)),
            "sourceUvOverlaps": overlap_summary(uv, [t["id"] for t in triangles], options.minimum_overlap_area),
            "fullReferencedSourceFaceUvOverlaps": overlap_summary(unique_uv,
                [{"sourceMesh": mesh, "sourceTriangle": face} for mesh, face in unique_sources], options.minimum_overlap_area),
            "areaUniformScaleCandidate": {"mmPerAtlasPixel": scale, "normalizedOriginAtlasPx": uv_min.tolist(),
                "normalizedBoundsMm": [[0., 0.], ((uv_max - uv_min) * scale).tolist()],
                "worldAreaMatchRatio": uv_sum * scale * scale / max(world_sum * 1e6, 1e-30),
                "distortion": distortion(world, uv, scale)}}
        if part_id in masks:
            mask = masks[part_id]
            source_entry = mask_sources.get(part_id, {})
            record["classification"] = {"source": source_entry.get("kind"),
                "file": source_entry.get("file", str((options.classification / "车身分件.psd").resolve())),
                "layer": source_entry.get("layer"), "sha256": source_entry.get("sha256"),
                "sourcePart": source_entry.get("sourceModelPart"),
                "layerTransform": source_entry.get("matrixFromPsdCanvasPixelIndices"),
                "originPx": source_entry.get("originPx"),
                "maskPixelCountAfterPartExclusions": int(mask.sum()),
                "candidateLocalUvBoundsPx": uv_min.tolist(),
                **classification_audit(mask, uv, world_area, size, source_mesh_geometries)}
        else:
            record["classification"] = {"source": None,
                "status": "requires-reviewed-PSD-layer-transform" if "fender" in part_id or part_id in ("su7.rear-bumper", "su7.charge-cover") else "no-classified-PNG-for-this-part"}
        report["parts"].append(record)
        print("CLASSIFIED_UV", part_id, "fragments", len(triangles), "overlapPairs", record["sourceUvOverlaps"]["positiveAreaPairCount"],
              "degenerate", record["degenerateSourceUvFragmentsAt1eMinus8Px2"], "mmPerPx", round(scale, 6), flush=True)
    report["totals"] = {"parts": len(groups), "fragments": len(document["triangles"]),
        "worldSurfaceAreaM2": total_world_area, "sourceUvAreaPx2": total_uv_area,
        "aggregateAreaScaleMmPerPixel": math.sqrt(total_world_area * 1e6 / total_uv_area),
        "positiveOverlapPairs": sum(p["sourceUvOverlaps"]["positiveAreaPairCount"] for p in report["parts"]),
        "degenerateSourceUvFragments": sum(p["degenerateSourceUvFragmentsAt1eMinus8Px2"] for p in report["parts"]),
        "classifiedMaskParts": sum(p["classification"]["source"] is not None for p in report["parts"]),
        "elapsedSeconds": time.monotonic() - started}
    options.output.parent.mkdir(parents=True, exist_ok=True)
    options.output.write_text(json.dumps(report, ensure_ascii=False, indent=2), encoding="utf-8")
    print("CLASSIFIED_UV_COMPLETE", options.output, json.dumps(report["totals"]), flush=True)


if __name__ == "__main__":
    main()
