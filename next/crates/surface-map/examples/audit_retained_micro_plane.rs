//! Public-API witness for a retained source-positive, f64-collinear micro mask.
use glam::{DMat2, DMat3, DVec2, DVec3, Vec2};
use itasha_surface_map::{CoverageFragment, PaintSurfaceMap, PaintTriangle, topology::Topology};
use serde_json::json;

fn world(t: &PaintTriangle) -> [DVec3; 3] {
    t.world.map(|p| DVec3::from_array(p.map(f64::from)))
}
fn area(w: [DVec3; 3]) -> f64 {
    (w[1] - w[0]).cross(w[2] - w[0]).length() * 5e5
}
fn original_normal(t: &PaintTriangle) -> DVec3 {
    let b = t.source_barycentric.map(|b| {
        let b = DVec3::from_array(b.map(f64::from));
        b / b.element_sum()
    });
    let w = world(t);
    let original =
        DMat3::from_cols(w[0], w[1], w[2]) * DMat3::from_cols(b[0], b[1], b[2]).inverse();
    (original.y_axis - original.x_axis)
        .cross(original.z_axis - original.x_axis)
        .normalize()
}
fn axes(f: &CoverageFragment) -> Option<([DVec3; 2], f64)> {
    let w: [DVec3; 3] = std::array::from_fn(|i| DVec3::from_array(f.world[i].map(f64::from)));
    let a = area(w);
    if a < 0.01 {
        return None;
    }
    let q: [DVec2; 3] =
        std::array::from_fn(|i| DVec2::from_array(f.image_uv[i].map(f64::from)) * 300.);
    let inverse = DMat2::from_cols(q[1] - q[0], q[2] - q[0]).inverse();
    if !inverse.is_finite() {
        return None;
    }
    let axis = |v: DVec2| ((w[1] - w[0]) * v.x + (w[2] - w[0]) * v.y) * 1000.;
    Some(([axis(inverse.x_axis), axis(inverse.y_axis)], a))
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let map: PaintSurfaceMap = serde_json::from_slice(&std::fs::read(&args[0])?)?;
    map.validate()?;
    let micro = map
        .triangles
        .iter()
        .find(|t| {
            t.id.source_mesh == "body.glb/node/175/primitive/0"
                && t.id.primitive == 0
                && t.id.source_triangle == 25455
                && t.id.fragment == 18446640630113940372
        })
        .unwrap();
    let stable = map
        .triangles
        .iter()
        .filter(|t| {
            t.id.source_mesh == micro.id.source_mesh
                && t.id.primitive == micro.id.primitive
                && t.id.source_triangle == micro.id.source_triangle
                && t.part_id == micro.part_id
        })
        .max_by(|a, b| area(world(a)).total_cmp(&area(world(b))))
        .unwrap();
    let topology = Topology::new(map.triangles.clone(), &map.connections);
    let at = micro
        .paint_uv
        .map(Vec2::from_array)
        .into_iter()
        .sum::<Vec2>()
        / 3.;
    let located = topology.locate(&micro.part_id, at);
    let frame = topology.tangent_frame(&micro.part_id, at);
    let coverage = topology.coverage_at(&micro.part_id, at.to_array(), [300., 300.], 0., false);
    let candidates: Vec<_> = coverage
        .iter()
        .filter(|f| {
            f.triangle.source_mesh == micro.id.source_mesh
                && f.triangle.primitive == micro.id.primitive
                && f.triangle.source_triangle == micro.id.source_triangle
        })
        .filter_map(|f| axes(f).map(|(a, s)| (f, a, s)))
        .collect();
    let measured = candidates.iter().max_by(|a, b| a.2.total_cmp(&b.2));
    let normal = original_normal(stable);
    let comparison=frame.zip(measured).map(|((origin,u,v),(f,axes,area))| {
        let public=[u.as_dvec3()*1000.,v.as_dvec3()*1000.];
        let angles:[f64;2]=std::array::from_fn(|i|public[i].normalize().dot(axes[i].normalize()).clamp(-1.,1.).acos().to_degrees());
        json!({"stableOutputTriangle":f.triangle.stable_key(),"stableOutputAreaMm2":area,"publicOrigin":origin.to_array(),"publicImageAxesWorldPerMm":[u.to_array(),v.to_array()],"publicAxisLengthsPerMm":public.map(|a|a.length()),"publicAxesDot":public[0].dot(public[1]),"publicNormalAbsDotOriginalSource":public[0].cross(public[1]).normalize().dot(normal).abs(),"measuredAxisLengthsPerMm":axes.map(|a|a.length()),"measuredAxesWorldPerMm":axes.map(|a|a.to_array()),"measuredNormalAbsDotOriginalSource":axes[0].cross(axes[1]).normalize().dot(normal).abs(),"imageAxisErrorDegrees":angles,"passes":angles.iter().all(|a|*a<1.)&&axes.iter().all(|a|(a.length()-1.).abs()<0.01)&&public.iter().all(|a|(a.length()-1.).abs()<0.01)&&public[0].dot(public[1]).abs()<0.001&&public[0].cross(public[1]).normalize().dot(normal).abs()>0.999})
    });
    let report = json!({"schema":"itasha-retained-micro-public-plane-audit-v1","map":args[0],"paintFingerprint":map.paint_fingerprint,"microTriangle":micro.id.stable_key(),"microSourceBarycentric":micro.source_barycentric,"microRawWorldAreaMm2":area(world(micro)),"microSourceCenter":at.to_array(),"locatedTriangle":located.map(|(i,_)|topology.triangles[i].id.stable_key()),"locatesRequestedMicro":located.is_some_and(|(i,_)|topology.triangles[i].id==micro.id),"stableReference":stable.id.stable_key(),"stableReferenceAreaMm2":area(world(stable)),"independentOriginalSourceNormal":normal.to_array(),"coverageFragments":coverage.len(),"substantialSameSourceOutputFragments":candidates.len(),"microItselfOutput":coverage.iter().any(|f|f.triangle==micro.id),"comparison":comparison});
    std::fs::write(&args[1], serde_json::to_vec_pretty(&report)?)?;
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}
