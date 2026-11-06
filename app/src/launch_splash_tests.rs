use super::*;

fn at(ms: u64) -> (f32, f32) {
    opacities(Duration::from_millis(ms))
}

#[test]
fn fades_in_holds_then_fades_out() {
    let (content, backdrop) = at(0);
    assert_eq!(content, 0.);
    assert_eq!(backdrop, 1.);

    let (content, _) = at(90);
    assert!(content > 0. && content < 1.);

    assert_eq!(at(400), (1., 1.));

    let (content, backdrop) = at(800);
    assert!(backdrop > 0. && backdrop < 1.);
    assert!(content <= backdrop);

    assert_eq!(at(2_000), (0., 0.));
}

#[test]
fn alpha_scales_with_opacity() {
    let color = ColorU::new(10, 20, 30, 200);
    assert_eq!(with_alpha(color, 0.5).a, 100);
    assert_eq!(with_alpha(color, 2.).a, 200);
    assert_eq!(with_alpha(color, -1.).a, 0);
}
