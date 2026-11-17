//! Reproject saved anchors by source identity, never by packed-chart proximity.
use crate::{
    PaintSurfaceMap, TriangleId,
    topology::{Topology, triangle_tangent_axes},
};
use glam::{DMat2, DVec2, DVec3, Vec2, Vec3};
use std::collections::HashMap;

type SourceKey = (String, u32, u32, String);

/// Explicitly reviewed classification correction on one original source face.
/// This never permits a nearest-face fallback or relaxes geometric checks.
pub struct SourcePartCorrection<'a> {
    pub source_mesh: &'a str,
    pub primitive: u32,
    pub source_triangle: u32,
    pub from_part: &'a str,
    pub to_part: &'a str,
}

pub struct MappingTransition<'a> {
    old: &'a PaintSurfaceMap,
    new: &'a PaintSurfaceMap,
    old_ids: HashMap<String, usize>,
    new_sources: HashMap<SourceKey, Vec<usize>>,
    new_topology: Topology,
    part_corrections: HashMap<SourceKey, String>,
}

#[derive(Debug)]
pub struct ReprojectedAnchor {
    pub triangle: TriangleId,
    pub part_id: String,
    pub region_id: String,
    pub barycentric: Vec3,
    pub paint_uv: Vec2,
    pub rotation_rad: f32,
    pub reflected: bool,
}

impl<'a> MappingTransition<'a> {
    pub fn new(old: &'a PaintSurfaceMap, new: &'a PaintSurfaceMap) -> Result<Self, String> {
        Self::with_part_corrections(old, new, &[])
    }

    pub fn with_part_corrections(
        old: &'a PaintSurfaceMap,
        new: &'a PaintSurfaceMap,
        corrections: &[SourcePartCorrection<'_>],
    ) -> Result<Self, String> {
        old.validate()?;
        new.validate()?;
        if old.model_fingerprint != new.model_fingerprint {
            return Err("source geometry changed; an anchor migration is unsafe".into());
        }
        let old_ids = old
            .triangles
            .iter()
            .enumerate()
            .map(|(i, t)| (t.id.stable_key(), i))
            .collect();
        let mut new_sources: HashMap<SourceKey, Vec<usize>> = HashMap::new();
        for (i, t) in new.triangles.iter().enumerate() {
            new_sources
                .entry((
                    t.id.source_mesh.clone(),
                    t.id.primitive,
                    t.id.source_triangle,
                    t.part_id.clone(),
                ))
                .or_default()
                .push(i);
        }
        let mut part_corrections = HashMap::new();
        for c in corrections {
            let source = (c.source_mesh.to_owned(), c.primitive, c.source_triangle);
            let key = (source.0.clone(), source.1, source.2, c.from_part.to_owned());
            let target = (source.0, source.1, source.2, c.to_part.to_owned());
            if c.from_part == c.to_part
                || !new_sources.contains_key(&target)
                || part_corrections.insert(key, c.to_part.to_owned()).is_some()
            {
                return Err("invalid or ambiguous source-part correction".into());
            }
        }
        Ok(Self {
            old,
            new,
            old_ids,
            new_sources,
            new_topology: Topology::new(new.triangles.clone(), &new.connections),
            part_corrections,
        })
    }

    pub fn reproject(
        &self,
        id: &str,
        part: &str,
        bary: Vec3,
        angle: f32,
    ) -> Result<ReprojectedAnchor, String> {
        let old = self
            .old_ids
            .get(id)
            .map(|&i| &self.old.triangles[i])
            .ok_or("saved paint fragment is absent from its predecessor")?;
        if old.part_id != part
            || !bary.is_finite()
            || (bary.element_sum() - 1.).abs() > 1e-4
            || bary.min_element() < -1e-4
            || !angle.is_finite()
        {
            return Err("saved paint anchor is invalid".into());
        }
        let mix = |values: [[f32; 3]; 3], b: DVec3| {
            let v = values.map(|p| DVec3::from_array(p.map(f64::from)));
            v[0] * b.x + v[1] * b.y + v[2] * b.z
        };
        let source_points = |t: &crate::PaintTriangle| {
            t.source_barycentric.map(|b| {
                let b = DVec3::from_array(b.map(f64::from));
                DVec2::new(b.y, b.z) / b.element_sum()
            })
        };
        let source = source_points(old);
        let point = source[0] * f64::from(bary.x)
            + source[1] * f64::from(bary.y)
            + source[2] * f64::from(bary.z);
        let old_world = mix(old.world, bary.as_dvec3());
        let mut key = (
            old.id.source_mesh.clone(),
            old.id.primitive,
            old.id.source_triangle,
            part.to_string(),
        );
        if let Some(corrected) = self.part_corrections.get(&key) {
            key.3 = corrected.clone();
        }
        let candidates = self
            .new_sources
            .get(&key)
            .ok_or("source face is outside the reviewed replacement paint mask")?;
        let mut best = None;
        for &i in candidates {
            let t = &self.new.triangles[i];
            let s = source_points(t);
            let m = DMat2::from_cols(s[1] - s[0], s[2] - s[0]);
            if m.determinant().abs() < 1e-20 {
                continue;
            }
            let b = m.inverse() * (point - s[0]);
            let b = DVec3::new(1. - b.x - b.y, b.x, b.y);
            if !b.is_finite() || b.min_element() < -2e-5 {
                continue;
            }
            let error = (mix(t.world, b) - old_world).length();
            if error > 0.00002 {
                continue;
            }
            // Prefer the interior of a triangle when an anchor lies on a split edge.
            if best
                .as_ref()
                .is_none_or(|(_, _, score): &(usize, DVec3, f64)| b.min_element() > *score)
            {
                best = Some((i, b, b.min_element()));
            }
        }
        let (i, b, _) =
            best.ok_or("source point is outside the reviewed replacement paint mask")?;
        let new = &self.new.triangles[i];
        let uv = new.paint_uv.map(|p| Vec2::from_array(p).as_dvec2());
        let paint_uv = (uv[0] * b.x + uv[1] * b.y + uv[2] * b.z).as_vec2();
        if self
            .new_topology
            .locate(&new.part_id, paint_uv)
            .map(|(index, _)| index)
            != Some(i)
        {
            return Err(
                "replacement anchor is not uniquely representable in its packed chart".into(),
            );
        }
        let (u, v) = triangle_tangent_axes(old).ok_or("predecessor tangent frame is invalid")?;
        let (tu, tv) = self
            .new_topology
            .fragment_tangent_axes(i)
            .ok_or("replacement tangent frame is invalid")?;
        let vertical = v * angle.cos() - u * angle.sin();
        let rotation_rad = (-tu.dot(vertical)).atan2(tv.dot(vertical));
        let reflected = u.cross(v).dot(tu.cross(tv)) < 0.;
        let horizontal = u * angle.cos() + v * angle.sin();
        let new_vertical = tv * rotation_rad.cos() - tu * rotation_rad.sin();
        let new_horizontal =
            (tu * rotation_rad.cos() + tv * rotation_rad.sin()) * if reflected { -1. } else { 1. };
        // Old f32 slivers can have a numerically corrupted normal even when the
        // source position is close. Refuse an inexact image-axis transport rather
        // than silently rotating an existing artwork out of its original plane.
        let min_axis_dot = 1_f32.to_radians().cos();
        if vertical.dot(new_vertical) < min_axis_dot
            || horizontal.dot(new_horizontal) < min_axis_dot
        {
            return Err(
                "saved fragment frame is too ill-conditioned to preserve image axes".into(),
            );
        }
        Ok(ReprojectedAnchor {
            triangle: new.id.clone(),
            part_id: new.part_id.clone(),
            region_id: new.region_id.clone(),
            barycentric: b.as_vec3(),
            paint_uv,
            rotation_rad,
            reflected,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PaintTriangle;
    fn map(triangles: Vec<PaintTriangle>) -> PaintSurfaceMap {
        let mut m = PaintSurfaceMap {
            mapping_version: 1,
            model_fingerprint: "model".into(),
            paint_fingerprint: String::new(),
            triangles,
            connections: vec![],
        };
        m.paint_fingerprint = m.fingerprint();
        m
    }
    #[test]
    fn repacked_and_split_fragments_preserve_source_position_and_image_axes() {
        let original = PaintTriangle {
            id: TriangleId {
                source_mesh: "body".into(),
                primitive: 0,
                source_triangle: 7,
                fragment: 3,
            },
            part_id: "panel".into(),
            region_id: "paint".into(),
            chart_id: "chart".into(),
            world: [[0., 0., 0.], [1., 0., 0.], [0., 1., 0.]],
            source_barycentric: [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]],
            paint_uv: [[0., 0.], [1000., 0.], [0., 1000.]],
        };
        let old = map(vec![original.clone()]);
        let mut a = original.clone();
        a.id.fragment = 4;
        a.world[2] = [0., 0.5, 0.];
        a.source_barycentric[2] = [0.5, 0., 0.5];
        let mut b = original;
        b.id.fragment = 5;
        b.world[0] = [0., 0.5, 0.];
        b.source_barycentric[0] = [0.5, 0., 0.5];
        for t in [&mut a, &mut b] {
            t.paint_uv = t.world.map(|p| {
                [
                    1300. + 300. * p[0] + 600. * p[1],
                    400. + 450. * p[0] - 100. * p[1],
                ]
            });
        }
        let new = map(vec![a, b]);
        let migration = MappingTransition::new(&old, &new).unwrap();
        let p = migration
            .reproject("body:0:7:3", "panel", Vec3::new(0.1, 0.2, 0.7), 0.47)
            .unwrap();
        assert_eq!(p.triangle.fragment, 5);
        assert!((p.paint_uv - Vec2::new(1780., 420.)).length() < 0.001);
        let (ou, ov) = triangle_tangent_axes(&old.triangles[0]).unwrap();
        let (nu, nv) = triangle_tangent_axes(&new.triangles[1]).unwrap();
        let before = ov * 0.47_f32.cos() - ou * 0.47_f32.sin();
        let after = nv * p.rotation_rad.cos() - nu * p.rotation_rad.sin();
        assert!(before.dot(after) > 0.999999);
        assert!(p.reflected);
        let before_x = ou * 0.47_f32.cos() + ov * 0.47_f32.sin();
        let after_x = (nu * p.rotation_rad.cos() + nv * p.rotation_rad.sin())
            * if p.reflected { -1. } else { 1. };
        assert!(before_x.dot(after_x) > 0.999999);
        assert!(
            migration
                .reproject("body:0:7:3", "other", Vec3::ONE / 3., 0.)
                .is_err()
        );
    }

    #[test]
    fn close_position_does_not_allow_a_corrupted_sliver_to_rotate_artwork() {
        let original = PaintTriangle {
            id: TriangleId {
                source_mesh: "body".into(),
                primitive: 0,
                source_triangle: 7,
                fragment: 3,
            },
            part_id: "panel".into(),
            region_id: "paint".into(),
            chart_id: "chart".into(),
            world: [[0., 0., 0.], [0.02, 0., 0.], [0.01, 1e-9, 1e-7]],
            source_barycentric: [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]],
            paint_uv: [[0., 0.], [20., 0.], [10., 0.1]],
        };
        let mut repaired = original.clone();
        repaired.world[2] = [0.01, 1e-7, 0.];
        let old = map(vec![original]);
        let new = map(vec![repaired]);
        let migration = MappingTransition::new(&old, &new).unwrap();
        let error = migration
            .reproject("body:0:7:3", "panel", Vec3::ONE / 3., 0.47)
            .unwrap_err();
        assert!(error.contains("image axes"), "{error}");
    }
}
