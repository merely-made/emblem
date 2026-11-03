use super::*;
use crate::sink::{Call, Paint};
use crate::{Host, Recorder, decode_metadata, execute};

/// The specification's worked example: Material Design's "action/info"
/// icon, 36 bytes, printed there with a full annotated disassembly.
const ACTION_INFO: [u8; 36] = [
    0x8a, 0x49, 0x56, 0x47, 0x03, 0x0b, 0x11, 0x51, 0x51, 0xb1, 0xb1, 0x35, 0x81, 0x59, 0x33, 0x59,
    0x81, 0x81, 0xa9, 0x35, 0x85, 0x95, 0x34, 0x7d, 0x95, 0x7d, 0x7d, 0x35, 0x85, 0x75, 0x34, 0x7d,
    0x75, 0x7d, 0x6d, 0x88,
];

fn view_box(min_x: f32, min_y: f32, max_x: f32, max_y: f32) -> ViewBox {
    ViewBox {
        min_x,
        min_y,
        max_x,
        max_y,
    }
}

fn run(file: &[u8]) -> Vec<Call> {
    let (_, start) = decode_metadata(file).expect("metadata");
    let mut sink = Recorder::default();
    execute(file, start, &Palette::default(), Host::default(), &mut sink).expect("execution");
    sink.calls
}

/// The icon, rebuilt through the encoder's public API.
fn action_info_writer() -> Writer {
    let mut w = Writer::new(view_box(-24.0, -24.0, 24.0, 24.0));
    w.move_to(0.0, -20.0).unwrap();
    w.ellipse(4, (-20.0, 0.0), (0.0, 20.0)).unwrap();
    w.move_to(2.0, 10.0).unwrap();
    w.parallelogram((-2.0, 10.0), (-2.0, -2.0)).unwrap();
    w.move_to(2.0, -6.0).unwrap();
    w.parallelogram((-2.0, -6.0), (-2.0, -10.0)).unwrap();
    w.fill_flat(8).unwrap();
    w
}

// ---- the specification's own example ------------------------------------

#[test]
fn the_spec_icon_is_reproduced_byte_for_byte() {
    assert_eq!(
        action_info_writer().finish().unwrap(),
        ACTION_INFO,
        "the encoder must reproduce the specification's 36 bytes exactly",
    );
}

#[test]
fn the_spec_icon_is_thirty_six_bytes() {
    assert_eq!(action_info_writer().finish().unwrap().len(), 36);
}

#[test]
fn the_rebuilt_icon_decodes_to_the_same_calls_as_the_original() {
    assert_eq!(
        run(&action_info_writer().finish().unwrap()),
        run(&ACTION_INFO)
    );
}

// ---- framing ------------------------------------------------------------

#[test]
fn a_file_starts_with_the_magic_identifier() {
    let bytes = Writer::new(ViewBox::default()).finish().unwrap();
    assert_eq!(&bytes[..MAGIC.len()], MAGIC);
}

#[test]
fn a_default_view_box_writes_no_chunk() {
    // An absent MID 8 already means (-32, -32, +32, +32).
    let bytes = Writer::new(ViewBox::default()).finish().unwrap();
    assert_eq!(
        bytes.len(),
        MAGIC.len() + 1,
        "magic plus a zero chunk count"
    );
    let (meta, _) = decode_metadata(&bytes).unwrap();
    assert_eq!(meta.view_box, ViewBox::default());
}

#[test]
fn a_custom_view_box_round_trips() {
    for vb in [
        view_box(-24.0, -24.0, 24.0, 24.0),
        view_box(0.0, 0.0, 1.0, 1.0),
        view_box(-1.5, -0.25, 100.0, 0.5),
    ] {
        let bytes = Writer::new(vb).finish().unwrap();
        let (meta, _) = decode_metadata(&bytes).unwrap();
        assert_eq!(meta.view_box, vb);
    }
}

#[test]
fn a_suggested_palette_round_trips() {
    let mut colors = Palette::default().0;
    colors[0] = Rgba::new(0xFF, 0x00, 0x00, 0xFF);
    colors[63] = Rgba::new(0x00, 0x80, 0x80, 0x80);
    let palette = Palette::new(colors).unwrap();

    let mut w = Writer::new(ViewBox::default());
    w.suggested_palette(palette);
    let bytes = w.finish().unwrap();

    let (meta, _) = decode_metadata(&bytes).unwrap();
    assert_eq!(meta.suggested_palette, palette);
}

#[test]
fn chunks_are_written_in_increasing_mid_order() {
    // The decoder refuses out-of-order chunks, so a file carrying both
    // proves the ordering by decoding at all.
    let mut w = Writer::new(view_box(-1.0, -1.0, 1.0, 1.0));
    w.suggested_palette(Palette::default());
    let bytes = w.finish().unwrap();
    let (meta, _) = decode_metadata(&bytes).unwrap();
    assert_eq!(meta.view_box, view_box(-1.0, -1.0, 1.0, 1.0));
    assert_eq!(meta.suggested_palette, Palette::default());
}

// ---- geometry -----------------------------------------------------------

#[test]
fn line_runs_round_trip_through_the_decoder() {
    let points: Vec<Point> = (0..5).map(|i| (i as f32, -(i as f32))).collect();
    let mut w = Writer::new(ViewBox::default());
    w.move_to(0.0, 0.0).unwrap();
    w.line_to(&points).unwrap();
    w.fill_flat(1).unwrap();

    let calls = run(&w.finish().unwrap());
    let lines: Vec<&Call> = calls
        .iter()
        .filter(|c| matches!(c, Call::LineTo(..)))
        .collect();
    assert_eq!(lines.len(), points.len());
    for (call, point) in lines.iter().zip(&points) {
        assert_eq!(**call, Call::LineTo(point.0, point.1));
    }
}

#[test]
fn quad_and_cube_runs_round_trip() {
    let mut w = Writer::new(ViewBox::default());
    w.move_to(0.0, 0.0).unwrap();
    w.quad_to(&[((1.0, 2.0), (3.0, 4.0))]).unwrap();
    w.cube_to(&[((1.0, 2.0), (3.0, 4.0), (5.0, 6.0))]).unwrap();
    w.fill_flat(1).unwrap();

    let calls = run(&w.finish().unwrap());
    assert!(calls.contains(&Call::QuadTo(1.0, 2.0, 3.0, 4.0)));
    assert!(calls.contains(&Call::CubeTo(1.0, 2.0, 3.0, 4.0, 5.0, 6.0)));
}

#[test]
fn a_run_of_fifteen_fits_in_one_op_and_sixteen_does_not() {
    let short: Vec<Point> = (0..15).map(|i| (i as f32, 0.0)).collect();
    let long: Vec<Point> = (0..16).map(|i| (i as f32, 0.0)).collect();

    let mut w = Writer::new(ViewBox::default());
    w.line_to(&short).unwrap();
    assert_eq!(w.bytecode()[0], 0x0F, "fifteen fits in the low four bits");

    let mut w = Writer::new(ViewBox::default());
    w.line_to(&long).unwrap();
    assert_eq!(w.bytecode()[0], 0x00, "sixteen needs the biased form");
    assert_eq!(w.bytecode()[1], 0x01, "a natural zero, meaning 16");
}

#[test]
fn long_runs_round_trip_with_every_point_intact() {
    for count in [1usize, 15, 16, 17, 40, 300] {
        let points: Vec<Point> = (0..count).map(|i| ((i % 60) as f32 - 30.0, 0.0)).collect();
        let mut w = Writer::new(ViewBox::default());
        w.move_to(0.0, 0.0).unwrap();
        w.line_to(&points).unwrap();
        w.fill_flat(1).unwrap();

        let calls = run(&w.finish().unwrap());
        let lines: Vec<Point> = calls
            .iter()
            .filter_map(|c| match c {
                Call::LineTo(x, y) => Some((*x, *y)),
                _ => None,
            })
            .collect();
        assert_eq!(lines, points, "run of {count}");
    }
}

#[test]
fn an_empty_run_is_refused() {
    let mut w = Writer::new(ViewBox::default());
    assert_eq!(w.line_to(&[]), Err(EncodeError::EmptyRun));
}

#[test]
fn ellipse_quarters_are_bounded() {
    let mut w = Writer::new(ViewBox::default());
    assert_eq!(
        w.ellipse(0, (0.0, 0.0), (1.0, 1.0)),
        Err(EncodeError::EllipseQuartersOutOfRange(0))
    );
    assert_eq!(
        w.ellipse(5, (0.0, 0.0), (1.0, 1.0)),
        Err(EncodeError::EllipseQuartersOutOfRange(5))
    );
    for q in 1..=4u8 {
        let mut w = Writer::new(ViewBox::default());
        w.ellipse(q, (20.0, 0.0), (0.0, 20.0)).unwrap();
        assert_eq!(w.bytecode()[0], 0x30 + q - 1);
    }
}

#[test]
fn each_ellipse_quarter_count_yields_that_many_cubics() {
    for q in 1..=4u8 {
        let mut w = Writer::new(ViewBox::default());
        w.move_to(0.0, -20.0).unwrap();
        w.ellipse(q, (20.0, 0.0), (0.0, 20.0)).unwrap();
        w.fill_flat(1).unwrap();
        let cubics = run(&w.finish().unwrap())
            .iter()
            .filter(|c| matches!(c, Call::CubeTo(..)))
            .count();
        assert_eq!(cubics, usize::from(q), "{q} quarters");
    }
}

// ---- geometry with styling ----------------------------------------------

#[test]
fn a_whole_graphic_fills_from_the_callers_palette() {
    let mut w = Writer::new(view_box(-8.0, -8.0, 8.0, 8.0));
    w.move_to(-4.0, -4.0).unwrap();
    w.line_to(&[(4.0, -4.0), (4.0, 4.0), (-4.0, 4.0)]).unwrap();
    w.fill_flat(8).unwrap();
    let bytes = w.finish().unwrap();

    for color in [
        Rgba::new(0xFF, 0x00, 0x00, 0xFF),
        Rgba::new(0x00, 0x00, 0xFF, 0xFF),
    ] {
        let mut colors = Palette::default().0;
        colors[0] = color;
        let palette = Palette::new(colors).unwrap();
        let (_, start) = decode_metadata(&bytes).unwrap();
        let mut sink = Recorder::default();
        execute(&bytes, start, &palette, Host::default(), &mut sink).unwrap();
        assert_eq!(sink.calls.last(), Some(&Call::Fill(Paint::Flat(color))));
    }
}

// ---- fuzz ---------------------------------------------------------------

#[test]
fn randomised_paths_round_trip_without_panicking() {
    // A small deterministic PRNG: the point is coverage, not randomness,
    // and a fixed sequence keeps failures reproducible.
    let mut state = 0x2026_0829_u64;
    let mut next = move || {
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1);
        ((state >> 33) as u32) as f32 / 4_294_967_296.0 * 120.0 - 60.0
    };

    for case in 0..200 {
        let count = (case % 12) + 1;
        let points: Vec<Point> = (0..count).map(|_| (next(), next())).collect();
        let mut w = Writer::new(view_box(-64.0, -64.0, 64.0, 64.0));
        w.move_to(points[0].0, points[0].1).unwrap();
        w.line_to(&points).unwrap();
        w.fill_flat(1).unwrap();
        let bytes = w.finish().unwrap();

        let calls = run(&bytes);
        let lines: Vec<Point> = calls
            .iter()
            .filter_map(|c| match c {
                Call::LineTo(x, y) => Some((*x, *y)),
                _ => None,
            })
            .collect();
        assert_eq!(lines.len(), points.len(), "case {case}");
        for (got, want) in lines.iter().zip(&points) {
            // Coordinates may be quantized by the four-byte form; the
            // error is bounded well below a hundredth of a unit here.
            assert!(
                (got.0 - want.0).abs() < 0.01 && (got.1 - want.1).abs() < 0.01,
                "case {case}: {got:?} vs {want:?}",
            );
        }
    }
}

// ---- level of detail ----------------------------------------------------

/// Run a file at a given rasterization height.
fn run_at_height(file: &[u8], height: f32) -> Vec<Call> {
    let (_, start) = decode_metadata(file).expect("metadata");
    let mut sink = Recorder::default();
    execute(
        file,
        start,
        &Palette::default(),
        Host {
            features: 0,
            height,
        },
        &mut sink,
    )
    .expect("execution");
    sink.calls
}

fn line_count(calls: &[Call]) -> usize {
    calls
        .iter()
        .filter(|c| matches!(c, Call::LineTo(..)))
        .count()
}

/// A two-arm icon: one line below height 32, three lines at or above it.
fn two_arm_icon() -> Vec<u8> {
    let mut w = Writer::new(view_box(-16.0, -16.0, 16.0, 16.0));
    w.level_of_detail(0.0, 32.0, |arm| {
        arm.move_to(0.0, 0.0)?;
        arm.line_to(&[(8.0, 8.0)])?;
        arm.fill_flat(8)
    })
    .unwrap();
    w.level_of_detail(32.0, f32::INFINITY, |arm| {
        arm.move_to(0.0, 0.0)?;
        arm.line_to(&[(1.0, 1.0), (2.0, 4.0), (8.0, 8.0)])?;
        arm.fill_flat(8)
    })
    .unwrap();
    w.finish().unwrap()
}

#[test]
fn a_two_arm_icon_draws_a_different_arm_at_each_extreme() {
    let file = two_arm_icon();
    let small = run_at_height(&file, 16.0);
    let large = run_at_height(&file, 64.0);

    assert_eq!(line_count(&small), 1, "the simple arm below the threshold");
    assert_eq!(line_count(&large), 3, "the detailed arm above it");
    assert_ne!(small, large, "the arms must actually differ");

    // Exactly one arm runs, so exactly one fill happens either way.
    let fills = |calls: &[Call]| calls.iter().filter(|c| matches!(c, Call::Fill(_))).count();
    assert_eq!(fills(&small), 1);
    assert_eq!(fills(&large), 1);
}

#[test]
fn the_arm_switches_exactly_at_the_threshold() {
    let file = two_arm_icon();
    let lines = |h: f32| line_count(&run_at_height(&file, h));
    // The range is half-open: [0, 32) takes the simple arm, 32 does not.
    assert_eq!(lines(31.9), 1);
    assert_eq!(lines(32.0), 3, "the upper bound is exclusive");
    assert_eq!(lines(0.0), 1, "the lower bound is inclusive");
}

#[test]
fn a_lod_arm_may_hold_several_ops_and_still_be_skipped_whole() {
    let mut w = Writer::new(ViewBox::default());
    w.move_to(0.0, 0.0).unwrap();
    w.level_of_detail(1000.0, 2000.0, |arm| {
        for i in 0..6 {
            arm.line_to(&[(i as f32, i as f32)])?;
        }
        Ok(())
    })
    .unwrap();
    w.line_to(&[(9.0, 9.0)]).unwrap();
    w.fill_flat(8).unwrap();
    let file = w.finish().unwrap();

    // Far below the range: the whole arm is skipped, leaving one line.
    let calls = run_at_height(&file, 10.0);
    let lines: Vec<&Call> = calls
        .iter()
        .filter(|c| matches!(c, Call::LineTo(..)))
        .collect();
    assert_eq!(lines.len(), 1, "only the line after the branch");
    assert_eq!(*lines[0], Call::LineTo(9.0, 9.0));

    // Inside it: the arm runs too.
    assert_eq!(line_count(&run_at_height(&file, 1500.0)), 7);
}

#[test]
fn nested_lod_branches_skip_correctly() {
    // An outer arm containing an inner branch: the outer op count must
    // cover the inner jump and its body as a unit.
    let mut w = Writer::new(ViewBox::default());
    w.move_to(0.0, 0.0).unwrap();
    w.level_of_detail(0.0, 100.0, |outer| {
        outer.line_to(&[(1.0, 1.0)])?;
        outer.level_of_detail(0.0, 50.0, |inner| inner.line_to(&[(2.0, 2.0)]))?;
        outer.line_to(&[(3.0, 3.0)])
    })
    .unwrap();
    w.fill_flat(8).unwrap();
    let file = w.finish().unwrap();

    let count = |h: f32| line_count(&run_at_height(&file, h));
    assert_eq!(count(25.0), 3, "outer and inner both run");
    assert_eq!(count(75.0), 2, "outer runs, inner is skipped");
    assert_eq!(count(150.0), 0, "the whole outer arm is skipped");
}

#[test]
fn an_arm_that_moves_sel_is_refused() {
    // A skipped arm never runs, so SEL after the branch would depend on the
    // host's height and every later slot would address a different register.
    let mut w = Writer::new(ViewBox::default());
    let err = w
        .level_of_detail(0.0, 32.0, |arm| {
            arm.adjust_sel(3);
            arm.line_to(&[(1.0, 1.0)])
        })
        .unwrap_err();
    assert!(
        matches!(err, EncodeError::LodArmChangesSel { .. }),
        "expected a SEL-drift refusal, got {err:?}",
    );

    // An arm that puts SEL back is fine.
    let mut w = Writer::new(ViewBox::default());
    w.level_of_detail(0.0, 32.0, |arm| {
        arm.adjust_sel(3);
        arm.adjust_sel(61); // 3 + 61 = 64, which is 0 modulo 64
        arm.line_to(&[(1.0, 1.0)])
    })
    .unwrap();
    assert_eq!(w.sel(), StyleWriter::new().sel(), "SEL unchanged outside");
}

#[test]
fn a_slot_zero_fill_inside_an_arm_is_caught() {
    // fill_flat(0) increments SEL, so an arm using it does not restore SEL.
    let mut w = Writer::new(ViewBox::default());
    let err = w
        .level_of_detail(0.0, 32.0, |arm| {
            arm.line_to(&[(1.0, 1.0)])?;
            arm.fill_flat(0)
        })
        .unwrap_err();
    assert!(matches!(
        err,
        EncodeError::LodArmChangesSel {
            entry: 56,
            exit: 57
        }
    ));
}

#[test]
fn inverted_and_empty_lod_ranges_are_refused() {
    let mut w = Writer::new(ViewBox::default());
    assert!(matches!(
        w.level_of_detail(32.0, 0.0, |arm| arm.line_to(&[(1.0, 1.0)])),
        Err(EncodeError::LodRangeInverted { .. })
    ));
    assert!(matches!(
        w.level_of_detail(f32::NAN, 1.0, |arm| arm.line_to(&[(1.0, 1.0)])),
        Err(EncodeError::NotANumber)
    ));
    assert!(matches!(
        w.level_of_detail(0.0, 32.0, |_| Ok(())),
        Err(EncodeError::EmptyRun)
    ));
}

#[test]
fn the_op_count_matches_what_was_written() {
    let mut w = Writer::new(ViewBox::default());
    w.move_to(0.0, 0.0).unwrap();
    w.line_to(&[(1.0, 1.0)]).unwrap();
    w.fill_flat(8).unwrap();
    assert_eq!(w.op_count(), 3);

    // A branch counts as the jump plus its arm.
    let mut w = Writer::new(ViewBox::default());
    w.level_of_detail(0.0, 32.0, |arm| {
        arm.move_to(0.0, 0.0)?;
        arm.line_to(&[(1.0, 1.0)])
    })
    .unwrap();
    assert_eq!(w.op_count(), 3, "one jump plus two arm ops");
}

#[test]
fn a_lod_icon_is_deterministic() {
    let once = two_arm_icon();
    for _ in 0..8 {
        assert_eq!(two_arm_icon(), once);
    }
}

// ---- determinism --------------------------------------------------------

#[test]
fn whole_file_encoding_is_deterministic() {
    let once = action_info_writer().finish().unwrap();
    for _ in 0..8 {
        assert_eq!(action_info_writer().finish().unwrap(), once);
    }
}

#[test]
fn a_richer_graphic_is_deterministic_too() {
    let build = || {
        let mut w = Writer::new(view_box(-16.0, -16.0, 16.0, 16.0));
        w.suggested_palette(Palette::default());
        for i in 0..8 {
            let f = i as f32;
            w.move_to(f, -f).unwrap();
            w.line_to(&[(f + 1.0, f), (f + 2.0, f + 1.0)]).unwrap();
            w.set_blend(1, ColorSource::Custom(i), ColorSource::Custom(i), 0, 0.0)
                .unwrap();
            w.fill_flat(1).unwrap();
        }
        w.finish().unwrap()
    };
    let once = build();
    for _ in 0..8 {
        assert_eq!(build(), once);
    }
}
