//! A real thin trunk fragment previously rotated a 300 mm image by 92.2 degrees.
//! This checks generated image axes on the same original source face, so a
//! curved vehicle surface cannot explain the discrepancy.
mod support;

use glam::{DMat2, DMat3, DVec2, DVec3, Vec2};
use itasha_surface_map::{CoverageFragment, PaintTriangle, topology::Topology};

fn world(t: &PaintTriangle) -> [DVec3; 3] {
    t.world.map(|p| DVec3::from_array(p.map(f64::from)))
}
fn area(w: [DVec3; 3]) -> f64 {
    (w[1] - w[0]).cross(w[2] - w[0]).length() * 0.5 * 1e6
}
fn normal(t: &PaintTriangle) -> DVec3 {
    let w = world(t);
    (w[1] - w[0]).cross(w[2] - w[0]).normalize()
}
fn angle(a: DVec3, b: DVec3) -> f64 {
    a.normalize()
        .dot(b.normalize())
        .clamp(-1., 1.)
        .acos()
        .to_degrees()
}
fn source_area(t: &PaintTriangle) -> f64 {
    let b = t.source_barycentric.map(|p| {
        let b = DVec3::from_array(p.map(f64::from));
        b / b.element_sum()
    });
    DMat3::from_cols(b[0], b[1], b[2]).determinant().abs() * 0.5
}
fn centroid(t: &PaintTriangle) -> Vec2 {
    t.paint_uv.map(Vec2::from_array).into_iter().sum::<Vec2>() / 3.
}
fn minimum_altitude_mm(t: &PaintTriangle) -> f64 {
    let w = world(t);
    let longest = (0..3)
        .map(|i| w[i].distance(w[(i + 1) % 3]))
        .fold(0., f64::max);
    2. * area(w) / (longest * 1000.)
}
// Independent derivative of generated image coordinates against world space.
fn image_axes(f: &CoverageFragment) -> Option<[DVec3; 2]> {
    let w: [DVec3; 3] = std::array::from_fn(|i| DVec3::from_array(f.world[i].map(f64::from)));
    if area(w) < 0.1 {
        return None;
    }
    let q: [DVec2; 3] = std::array::from_fn(|i| Vec2::from_array(f.image_uv[i]).as_dvec2() * 300.);
    let j = DMat2::from_cols(q[1] - q[0], q[2] - q[0]);
    if j.determinant().abs() < 1e-10 {
        return None;
    }
    let inverse = j.inverse();
    let apply = |v: DVec2| (w[1] - w[0]) * v.x + (w[2] - w[0]) * v.y;
    let axes = [apply(inverse.x_axis), apply(inverse.y_axis)];
    axes.iter().all(|a| a.is_finite()).then_some(axes)
}

#[test]
fn real_source_positive_micro_anchor_preserves_image_axes_on_stable_source_face() {
    let map = support::runtime_map();
    let topology = Topology::new(map.triangles.clone(), &map.connections);
    let face: Vec<_> = map
        .triangles
        .iter()
        .filter(|t| {
            t.id.source_mesh == "body.glb/node/91/primitive/0"
                && t.id.primitive == 0
                && t.id.source_triangle == 1818
                && t.part_id == "su7.trunk"
        })
        .collect();
    let reference = *face
        .iter()
        .max_by(|a, b| source_area(a).total_cmp(&source_area(b)))
        .expect("reviewed trunk source face 1818 must exist");
    assert!(
        area(world(reference)) > 1.,
        "source face must retain a stable reference fragment"
    );
    let reference_coverage = topology.coverage_at(
        &reference.part_id,
        centroid(reference).to_array(),
        [300., 300.],
        0.,
        false,
    );
    let reference_output = reference_coverage
        .iter()
        .filter(|f| f.triangle == reference.id)
        .find_map(image_axes)
        .expect("300 mm stable reference placement must cover its own large source fragment");
    let mut candidates: Vec<_> = face
        .into_iter()
        .filter(|t| {
            t.chart_id == reference.chart_id
                && source_area(t) > 0.
                && minimum_altitude_mm(t) < 0.001
                && angle(normal(t), normal(reference)) > 2.
        })
        .collect();
    candidates.sort_by(|a, b| {
        angle(normal(b), normal(reference)).total_cmp(&angle(normal(a), normal(reference)))
    });
    assert!(
        !candidates.is_empty(),
        "real source-positive thin regression fragments must remain in the reviewed mask"
    );
    let mut checked = 0;
    let mut isolated = 0;
    for t in candidates {
        let center = centroid(t);
        let Some((i, _)) = topology.locate(&t.part_id, center) else {
            continue;
        };
        if topology.triangles[i].id != t.id {
            continue;
        }
        let coverage = topology.coverage_at(&t.part_id, center.to_array(), [300., 300.], 0., false);
        let Some(output) = coverage
            .iter()
            .filter(|f| f.triangle == reference.id)
            .find_map(image_axes)
        else {
            isolated += 1;
            continue;
        };
        assert!(
            topology.tangent_frame(&t.part_id, center).is_some(),
            "micro anchor {} was explicitly rejected despite producing broad coverage",
            t.id.stable_key()
        );
        let errors = [
            angle(output[0], reference_output[0]),
            angle(output[1], reference_output[1]),
        ];
        assert!(
            errors.into_iter().all(|a| a <= 1.),
            "300 mm micro anchor {} rotates generated image X/Y by {errors:?} degrees on stable same-source output {}; source height={} mm, normal drift={} degrees",
            t.id.stable_key(),
            reference.id.stable_key(),
            minimum_altitude_mm(t),
            angle(normal(t), normal(reference))
        );
        checked += 1;
        if checked == 8 {
            break;
        }
    }
    assert!(
        checked > 0,
        "no locatable source-positive micro anchor reached the stable source face; isolated candidates={isolated}. Empty or refused placement does not certify image direction"
    );
    eprintln!(
        "verified {checked} real micro anchors against stable same-source output; isolated thin islands={isolated}"
    );
}

#[test]
fn source_positive_collinear_canonical_micro_anchor_has_the_same_frame_as_its_stable_output() {
    let map = support::runtime_map();
    let micro = map
        .triangles
        .iter()
        .find(|t| {
            t.id.source_mesh == "body.glb/node/175/primitive/0"
                && t.id.primitive == 0
                && t.id.source_triangle == 25455
                && t.id.fragment == 18446640630113940372
                && t.part_id == "su7.charge-cover"
        })
        .expect("retain the source-positive charge-cover micro mask");
    assert!(source_area(micro) > 0.);
    let reference = map
        .triangles
        .iter()
        .filter(|t| {
            t.id.source_mesh == micro.id.source_mesh
                && t.id.primitive == micro.id.primitive
                && t.id.source_triangle == micro.id.source_triangle
                && t.part_id == micro.part_id
        })
        .max_by(|a, b| source_area(a).total_cmp(&source_area(b)))
        .unwrap();
    assert!(area(world(reference)) > 1.);
    let b = reference.source_barycentric.map(|b| {
        let b = DVec3::from_array(b.map(f64::from));
        b / b.element_sum()
    });
    let w = world(reference);
    let original =
        DMat3::from_cols(w[0], w[1], w[2]) * DMat3::from_cols(b[0], b[1], b[2]).inverse();
    let original_normal = (original.y_axis - original.x_axis)
        .cross(original.z_axis - original.x_axis)
        .normalize();
    let topology = Topology::new(map.triangles.clone(), &map.connections);
    let at = centroid(micro);
    let (i, _) = topology
        .locate(&micro.part_id, at)
        .expect("micro anchor must locate");
    assert_eq!(topology.triangles[i].id, micro.id);
    let (_, u, v) = topology
        .tangent_frame(&micro.part_id, at)
        .expect("micro anchor must have a finite physical frame");
    let public = [u.as_dvec3(), v.as_dvec3()];
    let coverage = topology.coverage_at(&micro.part_id, at.to_array(), [300., 300.], 0., false);
    let measured = coverage
        .iter()
        .filter(|f| f.triangle == reference.id)
        .find_map(image_axes)
        .expect("micro placement must produce substantial stable same-source output");
    let errors = [angle(public[0], measured[0]), angle(public[1], measured[1])];
    assert!(
        errors.iter().all(|a| *a <= 1.),
        "public micro frame and independently differentiated stable output must agree: {errors:?} degrees"
    );
    for axis in public.into_iter().chain(measured) {
        assert!(
            (axis.length() * 1000. - 1.).abs() < 0.01,
            "source-positive micro placement must preserve physical millimetres"
        );
    }
    assert!(
        (public[0] * 1000.).dot(public[1] * 1000.).abs() < 0.001,
        "micro physical axes must be orthogonal"
    );
    assert!(
        public[0]
            .cross(public[1])
            .normalize()
            .dot(original_normal)
            .abs()
            > 0.999,
        "micro physical frame must remain on the independently restored original source plane"
    );
}
