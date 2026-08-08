//! Malformed input must fail, never panic.
//!
//! Every byte these tests feed the decoder is attacker-controlled in the case
//! that matters: an icon fetched over the network and decoded before anyone
//! looks at it. So the contract is not "valid files decode correctly" — it is
//! "no file, however crafted, panics, hangs, or allocates without bound".
//!
//! These are deterministic rather than random. A fuzzer that finds a crash on
//! Tuesday and not on Wednesday is not a regression test.

use iconvg::{Host, MAGIC, Palette, Recorder, decode_metadata, execute};

/// Decode `file` and return whether it was accepted. Never panics if the
/// decoder is correct — which is the whole assertion.
fn try_decode(file: &[u8]) -> bool {
    let Ok((_, start)) = decode_metadata(file) else {
        return false;
    };
    let mut sink = Recorder::default();
    execute(file, start, &Palette::default(), Host::default(), &mut sink).is_ok()
}

fn file_with(bytecode: &[u8]) -> Vec<u8> {
    let mut file = MAGIC.to_vec();
    file.push(0x01); // a natural zero: no metadata chunks
    file.extend_from_slice(bytecode);
    file
}

/// The specification's worked example, used as a mutation base.
const ACTION_INFO: [u8; 36] = [
    0x8a, 0x49, 0x56, 0x47, 0x03, 0x0b, 0x11, 0x51, 0x51, 0xb1, 0xb1, 0x35, 0x81, 0x59, 0x33, 0x59,
    0x81, 0x81, 0xa9, 0x35, 0x85, 0x95, 0x34, 0x7d, 0x95, 0x7d, 0x7d, 0x35, 0x85, 0x75, 0x34, 0x7d,
    0x75, 0x7d, 0x6d, 0x88,
];

#[test]
fn every_opcode_with_every_short_tail_is_survivable() {
    // 256 opcodes x 9 tail lengths x 4 filler bytes. Exhaustive over the
    // shapes that reach the op decoder at all, which is where the length
    // arithmetic lives.
    for opcode in 0u8..=255 {
        for tail in 0..9usize {
            for filler in [0x00u8, 0x01, 0x81, 0xFF] {
                let mut code = vec![opcode];
                code.extend(std::iter::repeat_n(filler, tail));
                let _ = try_decode(&file_with(&code));
            }
        }
    }
}

#[test]
fn single_byte_mutations_of_a_valid_file_are_survivable() {
    // Flip each byte of a known-good file to a set of awkward values. This is
    // the shape a real corruption takes, and it exercises paths a synthetic
    // opcode sweep never reaches: a valid header with one bad length, a valid
    // op with one bad coordinate.
    for position in 0..ACTION_INFO.len() {
        for value in [0x00u8, 0x01, 0x02, 0x3C, 0x3D, 0x7F, 0x80, 0xC0, 0xFF] {
            let mut file = ACTION_INFO;
            file[position] = value;
            let _ = try_decode(&file);
        }
    }
}

#[test]
fn truncation_at_every_length_is_survivable() {
    // Every prefix of a valid file. A decoder that reads one byte past its
    // bounds fails here and nowhere else.
    for length in 0..ACTION_INFO.len() {
        let _ = try_decode(&ACTION_INFO[..length]);
    }
}

#[test]
fn a_crafted_indirect_segref_cannot_panic_on_its_offset_arithmetic() {
    // An Absolute Indirect SegRef packs a 55-bit file offset that the VM uses
    // to read 16 further bytes. The offset is attacker-controlled and enormous
    // relative to any real file, so the read must be bounds-checked rather
    // than computed and then indexed.
    for shift in 0..55u32 {
        let mut raw = 1u64 << 63; // the indirect marker
        raw |= (1u64 << shift) << 8; // a wild offset
        let mut code = vec![0x3Cu8];
        code.extend_from_slice(&raw.to_le_bytes());
        let accepted = try_decode(&file_with(&code));
        assert!(!accepted, "a segref pointing outside the file must be refused");
    }
}

#[test]
fn a_call_into_its_own_bytes_terminates() {
    // A segment whose range overlaps the Call op itself is explicitly legal:
    // "That start and end may overlap with the top level bytecode". It must
    // still terminate, because calls cannot nest and jumps only go forward.
    for length in 0u64..24 {
        let mut code = vec![0x3Cu8];
        code.extend_from_slice(&(length << 8).to_le_bytes());
        code.extend_from_slice(&[0x37, 0x37, 0x37, 0x88, 0x35, 0x81, 0x81]);
        let _ = try_decode(&file_with(&code));
    }
}

#[test]
fn a_jump_past_the_end_of_the_file_is_refused_or_ends() {
    // "It is valid to jump to the EOB or the end of the file exactly, but
    // invalid to jump past either." Either way it must not read past the end.
    for count in [0x01u8, 0x03, 0x05, 0x21, 0x81, 0xFF] {
        for opcode in [0x38u8, 0x39, 0x3A] {
            let mut code = vec![opcode, count];
            code.extend_from_slice(&[0x81, 0x81, 0x81, 0x81]);
            let _ = try_decode(&file_with(&code));
        }
    }
}

#[test]
fn arbitrary_bytecode_never_panics() {
    // Pseudo-random bytecode, deterministic across runs. The opcode sweep
    // above covers each op in isolation; this covers ops feeding each other,
    // where one op's operands become the next op's opcode.
    for seed in 0u32..2048 {
        let code: Vec<u8> = (0..48u32)
            .map(|i| {
                (seed
                    .wrapping_mul(2_654_435_761)
                    .wrapping_add(i.wrapping_mul(40_503))
                    >> 8) as u8
            })
            .collect();
        let _ = try_decode(&file_with(&code));
    }
}

#[test]
fn a_gradient_with_a_wild_stop_count_is_bounded() {
    // The Gradient Configuration's low six bits give up to 62, plus two, so a
    // fill can name 64 registers from a base of SEL+LOW4. Register indexing is
    // modulo 64, so this must wrap rather than index out of bounds.
    for config in 0u8..=255 {
        let mut code = vec![0x35u8, 0x81, 0x81]; // MoveTo(0,0)
        code.push(0x9F); // linear gradient, LOW4 = 15
        code.push(config);
        code.extend(std::iter::repeat_n(0x00, 12)); // three float32s
        let _ = try_decode(&file_with(&code));
    }
}
