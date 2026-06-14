// IMA-ADPCM (mono, 4-bit). Mirror of tools/adpcm.py — low nibble first,
// initial predictor 0 / step index 0. Change with src/audio/adpcm.c or neither.

const STEP_TABLE: [i32; 89] = [
    7, 8, 9, 10, 11, 12, 13, 14, 16, 17, 19, 21, 23, 25, 28, 31,
    34, 37, 41, 45, 50, 55, 60, 66, 73, 80, 88, 97, 107, 118, 130, 143,
    157, 173, 190, 209, 230, 253, 279, 307, 337, 371, 408, 449, 494, 544, 598, 658,
    724, 796, 876, 963, 1060, 1166, 1282, 1411, 1552, 1707, 1878, 2066, 2272, 2499,
    2749, 3024, 3327, 3660, 4026, 4428, 4871, 5358, 5894, 6484, 7132, 7845, 8630,
    9493, 10442, 11487, 12635, 13899, 15289, 16818, 18500, 20350, 22385, 24623,
    27086, 29794, 32767,
];
const INDEX_TABLE: [i32; 16] = [-1, -1, -1, -1, 2, 4, 6, 8, -1, -1, -1, -1, 2, 4, 6, 8];

/// Shared decoder-side state update. Integer math mirrors adpcm.c exactly.
fn step(predictor: i32, step_index: i32, nibble: u8) -> (i32, i32) {
    let s = STEP_TABLE[step_index as usize];
    let mut diff = s >> 3;
    if nibble & 1 != 0 { diff += s >> 2; }
    if nibble & 2 != 0 { diff += s >> 1; }
    if nibble & 4 != 0 { diff += s; }
    let mut p = if nibble & 8 != 0 { predictor - diff } else { predictor + diff };
    p = p.clamp(-32768, 32767);
    let si = (step_index + INDEX_TABLE[nibble as usize]).clamp(0, 88);
    (p, si)
}

pub fn encode(pcm: &[i16]) -> Vec<u8> {
    let (mut predictor, mut step_index) = (0i32, 0i32);
    let mut out: Vec<u8> = Vec::new();
    let mut low = true;
    for &sample in pcm {
        let mut delta = sample as i32 - predictor;
        let mut nib: u8 = if delta < 0 { 8 } else { 0 };
        if delta < 0 { delta = -delta; }
        let s = STEP_TABLE[step_index as usize];
        if delta >= s { nib |= 4; delta -= s; }
        if delta >= s >> 1 { nib |= 2; delta -= s >> 1; }
        if delta >= s >> 2 { nib |= 1; }
        let (p, si) = step(predictor, step_index, nib);
        predictor = p; step_index = si;
        if low { out.push(nib); } else { *out.last_mut().unwrap() |= nib << 4; }
        low = !low;
    }
    out
}

pub fn decode(data: &[u8], n: usize) -> Vec<i16> {
    let (mut predictor, mut step_index) = (0i32, 0i32);
    let mut out = Vec::with_capacity(n);
    for i in 0..n.min(data.len() * 2) {
        let byte = data[i >> 1];
        let nib = if i & 1 != 0 { byte >> 4 } else { byte & 0x0F };
        let (p, si) = step(predictor, step_index, nib);
        predictor = p; step_index = si;
        out.push(predictor as i16);
    }
    out
}

/// Decoder state (predictor, step_index) after each prefix length in
/// `byte_offsets` (ascending). Single pass over the nibble stream.
/// Returns (predictor as i16, step_index as u8) for index entries.
pub fn scan_states(blob: &[u8], byte_offsets: &[usize]) -> Vec<(i16, u8)> {
    let (mut predictor, mut step_index) = (0i32, 0i32);
    let mut states = Vec::with_capacity(byte_offsets.len());
    let mut wi = 0usize;
    for pos in 0..=blob.len() {
        while wi < byte_offsets.len() && byte_offsets[wi] == pos {
            states.push((predictor as i16, step_index as u8));
            wi += 1;
        }
        if pos == blob.len() { break; }
        let b = blob[pos];
        let (p, si) = step(predictor, step_index, b & 0x0F);
        let (p, si) = step(p, si, b >> 4);
        predictor = p; step_index = si;
    }
    while wi < byte_offsets.len() { // offsets past the end clamp
        states.push((predictor as i16, step_index as u8));
        wi += 1;
    }
    states
}
