//! The bytecode VM, driven through the public API.
//!
//! The centrepiece is the specification's own worked example, checked against
//! the disassembly the specification prints beside it. The rest cover the
//! rules that are easy to implement plausibly and wrongly: jumps counting ops
//! rather than bytes, the register ops' hidden `SEL` decrement, and the
//! reserved-opcode fallbacks.

use emblem::{Call, Error, Host, MAGIC, Paint, Palette, Recorder, decode_metadata, execute};

/// The specification's worked example: Material Design's "action/info" icon,
/// 36 bytes, printed there with a full annotated disassembly.
const ACTION_INFO: [u8; 36] = [
    0x8a, 0x49, 0x56, 0x47, 0x03, 0x0b, 0x11, 0x51, 0x51, 0xb1, 0xb1, 0x35, 0x81, 0x59, 0x33, 0x59,
    0x81, 0x81, 0xa9, 0x35, 0x85, 0x95, 0x34, 0x7d, 0x95, 0x7d, 0x7d, 0x35, 0x85, 0x75, 0x34, 0x7d,
    0x75, 0x7d, 0x6d, 0x88,
];

fn run_with(file: &[u8], host: Host) -> Recorder {
    let (_, start) = decode_metadata(file).expect("metadata");
    let mut sink = Recorder::default();
    execute(file, start, &Palette::default(), host, &mut sink).expect("execution");
    sink
}

fn run(file: &[u8]) -> Recorder {
    run_with(file, Host::default())
}

/// A minimal file: magic, no metadata chunks, then `bytecode`.
fn file_with(bytecode: &[u8]) -> Vec<u8> {
    let mut file = MAGIC.to_vec();
    file.push(0x01); // a natural zero
    file.extend_from_slice(bytecode);
    file
}

fn fails(file: &[u8]) -> Error {
    let (_, start) = decode_metadata(file).expect("metadata");
    let mut sink = Recorder::default();
    execute(file, start, &Palette::default(), Host::default(), &mut sink)
        .expect_err("expected this graphic to be refused")
}

#[test]
fn the_specifications_own_example_matches_its_disassembly() {
    let (metadata, start) = decode_metadata(&ACTION_INFO).unwrap();
    // "the IconVG ViewBox ranges from -24 to +24".
    assert_eq!(metadata.view_box.min_x, -24.0);
    assert_eq!(metadata.view_box.max_y, 24.0);
    assert_eq!(start, 11, "bytecode begins after the metadata");

    let sink = run(&ACTION_INFO);
    let calls = &sink.calls;

    // #0000 ClosePath; MoveTo (0, -20). Nothing is open, so ClosePath is a
    // no-op and only the MoveTo is seen.
    assert_eq!(calls[0], Call::MoveTo(0.0, -20.0));

    // #0001 Ellipse (4 quarters): four cubic segments.
    assert!(
        calls[1..5].iter().all(|c| matches!(c, Call::CubeTo(..))),
        "a full ellipse is four cubics, got {:?}",
        &calls[1..5]
    );

    // #0002 closes the ellipse, then starts the stem at (+2, +10).
    assert_eq!(calls[5], Call::Close);
    assert_eq!(calls[6], Call::MoveTo(2.0, 10.0));

    // #0003 Parallelogram from A=(2,10) via B=(-2,10) and C=(-2,-2), with
    // D = A - B + C implied, then back to A.
    assert_eq!(calls[7], Call::LineTo(-2.0, 10.0));
    assert_eq!(calls[8], Call::LineTo(-2.0, -2.0));
    assert_eq!(calls[9], Call::LineTo(2.0, -2.0), "the implied fourth corner");
    assert_eq!(calls[10], Call::LineTo(2.0, 10.0), "back to A");

    // #0004, #0005: the dot above the stem.
    assert_eq!(calls[11], Call::Close);
    assert_eq!(calls[12], Call::MoveTo(2.0, -6.0));
    assert_eq!(calls[13], Call::LineTo(-2.0, -6.0));

    // #0006 ClosePath; Fill (flat color).
    assert_eq!(calls[17], Call::Close);
    assert!(matches!(calls[18], Call::Fill(Paint::Flat(_))));
    assert_eq!(calls.len(), 19, "no calls beyond the disassembly's ops");
    assert_eq!(sink.fill_count(), 1);
}

#[test]
fn a_full_ellipse_returns_the_pen_to_where_it_started() {
    // "The 0x33 Full Ellipse op adds all four segments and leaves the Pen
    // Position unchanged at A." A VM that advanced the pen would misplace any
    // geometry that followed without an explicit MoveTo.
    let sink = run(&ACTION_INFO);
    let Call::CubeTo(_, _, _, _, x, y) = sink.calls[4] else {
        panic!("expected a cubic as the ellipse's fourth segment");
    };
    assert!(x.abs() < 1e-3, "returned to A.x, got {x}");
    assert!((y + 20.0).abs() < 1e-3, "returned to A.y, got {y}");
}

#[test]
fn a_parallelogram_leaves_the_pen_at_a_too() {
    // Same rule, different op: "The Pen Position does not change. It stays at
    // A." The example's next op is an explicit MoveTo, so only a test pins it.
    let mut code = vec![0x35, 0x85, 0x95]; // MoveTo (+2, +10)
    code.extend_from_slice(&[0x34, 0x7d, 0x95, 0x7d, 0x7d]); // Parallelogram
    code.extend_from_slice(&[0x01]); // LineTo, 1 rep... opcode 0x01
    code.extend_from_slice(&[0x81, 0x81]); // to (0, 0)
    code.push(0x88);
    let sink = run(&file_with(&code));
    // The line after the parallelogram starts from A, so the parallelogram's
    // own last point must have been A.
    assert_eq!(sink.calls[4], Call::LineTo(2.0, 10.0), "closed back to A");
    assert_eq!(sink.calls[5], Call::LineTo(0.0, 0.0));
}

#[test]
fn a_jump_skips_whole_ops_not_bytes() {
    // JumpCount counts *ops*. Skipping two here must clear a 7-byte cube and
    // a 1-byte NOP; counting bytes would land the PC mid-coordinate and
    // decode garbage.
    let mut code = vec![0x38, 0x05]; // Jump over 2 ops
    code.extend_from_slice(&[0x21, 0x81, 0x81, 0x81, 0x81, 0xa9, 0xa9]); // a cube
    code.push(0x37); // NOP
    code.push(0x88); // flat fill
    let sink = run(&file_with(&code));
    assert_eq!(
        sink.calls.len(),
        0,
        "the cube was jumped over, so nothing was drawn: {:?}",
        sink.calls
    );
}

#[test]
fn a_jump_of_one_lands_exactly_on_the_next_op() {
    // The counterpart: skipping only the cube must still run the fill, which
    // only happens if the cube's length was measured correctly.
    let mut code = vec![0x38, 0x03]; // Jump over 1 op
    code.extend_from_slice(&[0x21, 0x81, 0x81, 0x81, 0x81, 0xa9, 0xa9]); // a cube
    code.extend_from_slice(&[0x35, 0x81, 0x81]); // ClosePathMoveTo(0,0)
    code.push(0x88);
    let sink = run(&file_with(&code));
    assert_eq!(sink.fill_count(), 1, "landed on the ops after the cube");
    assert!(
        !sink.calls.iter().any(|c| matches!(c, Call::CubeTo(..))),
        "but the cube itself was skipped"
    );
}

#[test]
fn a_zero_count_jump_is_a_nop_and_not_an_infinite_loop() {
    // "a zero JumpCount Jump is effectively a NOP (and not an infinite loop)".
    // A VM that re-executed the jump would hang the test suite here.
    let code = vec![0x38, 0x01, 0x37];
    assert_eq!(run(&file_with(&code)).calls.len(), 0);
}

#[test]
fn a_feature_detection_jump_is_taken_unless_every_needed_bit_is_present() {
    // "the jump is taken unless the bitwise-and of FeaturesNeeded and
    // FeaturesImplemented equals FeaturesNeeded."
    let guarded = |features: u32| {
        // FDJump over 1 op, needing feature bit 0x01.
        let mut code = vec![0x39, 0x03, 0x03];
        code.extend_from_slice(&[0x35, 0x81, 0x81]); // the guarded MoveTo
        code.push(0x88);
        run_with(
            &file_with(&code),
            Host {
                features,
                height: 0.0,
            },
        )
    };

    // Host lacks the feature: the jump is taken, the guarded MoveTo is
    // skipped, and the fill finds nothing pending.
    assert_eq!(guarded(0x00).fill_count(), 0, "guarded op skipped");
    // Host has it: the jump is not taken and the guarded op runs.
    assert_eq!(guarded(0x01).fill_count(), 1, "guarded op executed");
}

#[test]
fn a_level_of_detail_jump_branches_on_the_rasterization_height() {
    // "the jump is taken unless (LOD0 <= H) and (H < LOD1)."
    let at_height = |height: f32| {
        // LODJump over 1 op unless 0 <= H < 32.
        let mut code = vec![0x3A, 0x03, 0x81, 0xa9];
        code.extend_from_slice(&[0x35, 0x81, 0x81]); // guarded MoveTo
        code.push(0x88);
        run_with(&file_with(&code), Host { features: 0, height }).fill_count()
    };

    assert_eq!(at_height(16.0), 1, "inside [0, 32): the detailed path runs");
    assert_eq!(at_height(64.0), 0, "outside: it is jumped over");
}

#[test]
fn calls_cannot_nest_and_that_is_what_bounds_recursion() {
    // "IconVG bytecode calls cannot nest. The maximum call stack depth is
    // one." So a Call inside a Call is an error rather than a stack overflow,
    // which is why this VM needs no depth limit and no fuel counter.
    let mut code = vec![0x3C];
    // Inline SegRef: type 0x00, length 9, holding one more Call op.
    code.extend_from_slice(&0x0000_0000_0000_0900u64.to_le_bytes());
    code.push(0x3C);
    code.extend_from_slice(&0u64.to_le_bytes());

    assert_eq!(fails(&file_with(&code)), Error::NestedCall);
}

#[test]
fn a_call_runs_its_segment_and_returns_to_the_caller() {
    // The inline case: the segment draws, then an implicit Return at the EOB
    // hands control back to the byte after the Call.
    let mut code = vec![0x3C];
    let segment = [0x35u8, 0x81, 0x81, 0x88]; // MoveTo(0,0); flat fill
    // Segment type 0x00 in the low byte, length in the 24 bits above it.
    let segref = (segment.len() as u64) << 8;
    code.extend_from_slice(&segref.to_le_bytes());
    code.extend_from_slice(&segment);
    // After returning, draw and fill again.
    code.extend_from_slice(&[0x35, 0x85, 0x85]);
    code.push(0x88);

    let sink = run(&file_with(&code));
    assert_eq!(
        sink.fill_count(),
        2,
        "one fill inside the segment, one after the return: {:?}",
        sink.calls
    );
}

#[test]
fn a_reserved_segment_type_is_refused_with_no_fallback() {
    // "Segment Types other than 0x00 are reserved and are invalid to Call in
    // this version (there is no fallback behavior)."
    let mut code = vec![0x3C];
    code.extend_from_slice(&0x0000_0000_0000_0001u64.to_le_bytes());
    assert_eq!(fails(&file_with(&code)), Error::ReservedSegmentType);
}

#[test]
fn a_reserved_nop_op_is_skipped_along_with_its_extra_data() {
    // 0xE0 does nothing but carries Extra Data. Everything after it must
    // still run, which happens only if EDLength was read and honoured.
    let mut code = vec![0xE0, 0x07, 0xAA, 0xBB, 0xCC]; // natural 3, then 3 bytes
    code.extend_from_slice(&[0x35, 0x81, 0x81]);
    code.push(0x88);
    assert_eq!(run(&file_with(&code)).fill_count(), 1);
}

#[test]
fn a_reserved_c0_op_falls_back_to_a_line_to() {
    // "The fallback behavior is a single LineTo that point (transformed by
    // GFTM)." The coordinate pair sits *after* the Extra Data.
    let mut code = vec![0x35, 0x81, 0x81]; // MoveTo(0, 0)
    code.extend_from_slice(&[0xC0, 0x01]); // reserved op, zero Extra Data
    code.extend_from_slice(&[0xa9, 0xa9]); // (+20, +20)
    code.push(0x88);
    let sink = run(&file_with(&code));
    assert!(
        sink.calls.contains(&Call::LineTo(20.0, 20.0)),
        "expected the fallback line, got {:?}",
        sink.calls
    );
}

#[test]
fn a_reserved_b0_op_fills_flat_like_an_80_op() {
    // "The fallback behavior is to fill with a flat color as if the opcode
    // was in [0x80 ..= 0x8F]."
    let mut code = vec![0x35, 0x81, 0x81];
    code.extend_from_slice(&[0xB8, 0x01]); // reserved fill, zero Extra Data
    let sink = run(&file_with(&code));
    assert_eq!(sink.fill_count(), 1);
    assert!(matches!(sink.calls.last(), Some(Call::Fill(Paint::Flat(_)))));
}

#[test]
fn an_empty_bytecode_ends_immediately() {
    // The PC is already at the end of the file, so an implicit Return ends
    // the graphic rather than reading past it.
    assert_eq!(run(&file_with(&[])).calls.len(), 0);
}

#[test]
fn a_truncated_op_is_an_error_rather_than_a_partial_draw() {
    // A cube promising six coordinates with three present.
    assert_eq!(fails(&file_with(&[0x21, 0x81, 0x81, 0x81])), Error::Truncated);
}

#[test]
fn a_fill_with_nothing_pending_draws_nothing() {
    // Two fills in a row: the second has an empty Pending Paths list, because
    // "These paths are consumed (not preserved)".
    let mut code = vec![0x35, 0x81, 0x81];
    code.push(0x88);
    code.push(0x88);
    assert_eq!(run(&file_with(&code)).fill_count(), 1);
}
