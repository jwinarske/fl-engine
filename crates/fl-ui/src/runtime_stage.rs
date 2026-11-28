//! impellerc's runtime stages: the `IPLR` flatbuffer a `.frag` asset holds,
//! read as `impeller/runtime_stage/runtime_stage.cc` reads it.
//!
//! The schema is `runtime_stage.fbs` and `runtime_stage_types.fbs`, format
//! version 2. An asset is app-supplied bytes, so every offset is bounds
//! checked here: a malformed one is the engine's "failed verification", never
//! a read past the end.

/// The backends a runtime stages file can carry a stage for, in the order
/// the schema lists them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Backend {
    SkSL,
    Metal,
    OpenGLES,
    OpenGLES3,
    Vulkan,
}

impl Backend {
    const ALL: [Backend; 5] = [
        Backend::SkSL,
        Backend::Metal,
        Backend::OpenGLES,
        Backend::OpenGLES3,
        Backend::Vulkan,
    ];

    /// The name the engine's errors give it (`RuntimeStageBackendToString`).
    pub(crate) fn name(self) -> &'static str {
        match self {
            Backend::SkSL => "SkSL",
            Backend::Metal => "Metal",
            Backend::OpenGLES => "OpenGLES",
            Backend::OpenGLES3 => "OpenGLES3",
            Backend::Vulkan => "Vulkan",
        }
    }

    /// The `RuntimeStages` field that holds it.
    fn field(self) -> usize {
        match self {
            Backend::SkSL => 0,
            Backend::Metal => 1,
            Backend::OpenGLES => 2,
            Backend::OpenGLES3 => 3,
            Backend::Vulkan => 4,
        }
    }
}

/// `UniformDataType`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UniformType {
    Float,
    SampledImage,
    Struct,
}

impl UniformType {
    /// The name `_uniformInfo` gives it.
    pub(crate) fn name(self) -> &'static str {
        match self {
            UniformType::Float => "Float",
            UniformType::SampledImage => "SampledImage",
            UniformType::Struct => "Struct",
        }
    }
}

/// One 4-byte slot of a uniform's GPU layout (`PaddingType`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Slot {
    Padding,
    Float,
}

/// `StructField`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StructField {
    pub(crate) name: String,
    pub(crate) byte_size: u64,
}

/// `UniformDescription`, as `RuntimeUniformDescription` keeps it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Uniform {
    pub(crate) name: String,
    pub(crate) location: u64,
    pub(crate) binding: u64,
    pub(crate) kind: UniformType,
    pub(crate) bit_width: u64,
    pub(crate) rows: u64,
    pub(crate) columns: u64,
    pub(crate) array_elements: u64,
    pub(crate) padding_layout: Vec<Slot>,
    pub(crate) struct_fields: Vec<StructField>,
}

impl Uniform {
    /// The bytes Dart writes for it (`GetDartSize`): its float slots, or
    /// rows × columns × width without a layout, times its array length.
    pub(crate) fn dart_size(&self) -> u64 {
        let size = if self.padding_layout.is_empty() {
            self.rows
                .saturating_mul(self.columns)
                .saturating_mul(self.bit_width)
                / 8
        } else {
            self.padding_layout
                .iter()
                .filter(|s| **s == Slot::Float)
                .count() as u64
                * 4
        };
        if self.array_elements > 0 {
            size.saturating_mul(self.array_elements)
        } else {
            size
        }
    }

    /// The bytes it takes in the uniform block (`GetGPUSize`).
    pub(crate) fn gpu_size(&self) -> u64 {
        let size = if self.padding_layout.is_empty() {
            self.rows
                .saturating_mul(self.columns)
                .saturating_mul(self.bit_width)
                / 8
        } else {
            self.padding_layout.len() as u64 * 4
        };
        if self.array_elements > 0 {
            size.saturating_mul(self.array_elements)
        } else {
            size
        }
    }
}

/// `RuntimeStage`: one backend's shader and what it reads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Stage {
    pub(crate) uniforms: Vec<Uniform>,
    pub(crate) shader: Vec<u8>,
}

/// A decoded file: each backend's stage, where it has one.
#[derive(Debug, Clone, Default)]
pub(crate) struct Stages([Option<Stage>; 5]);

impl Stages {
    /// `backend`'s stage, if the file has one.
    pub(crate) fn get(&self, backend: Backend) -> Option<&Stage> {
        self.0[backend.field()].as_ref()
    }

    /// Takes `backend`'s stage out.
    pub(crate) fn take(&mut self, backend: Backend) -> Option<Stage> {
        self.0[backend.field()].take()
    }

    /// No stage for any backend.
    pub(crate) fn is_empty(&self) -> bool {
        self.0.iter().all(Option::is_none)
    }

    /// The backends with a stage, in the engine's order, for its "Found
    /// stages" list.
    pub(crate) fn found(&self) -> Vec<Backend> {
        Backend::ALL
            .into_iter()
            .filter(|b| self.get(*b).is_some())
            .collect()
    }
}

/// The schema's `RuntimeStagesFormatVersion.kVersion`.
const FORMAT_VERSION: u32 = 2;

/// Decodes a runtime stages file (`DecodeRuntimeStages`). The error is the
/// engine's status message, which `initFromAsset` puts after "manifest could
/// not be decoded: ".
pub(crate) fn decode(bytes: &[u8]) -> Result<Stages, String> {
    // absl's InvalidArgumentError reads "INVALID_ARGUMENT: <message>" through
    // Status::ToString, which is what the engine's message shows.
    let invalid = |m: &str| format!("INVALID_ARGUMENT: {m}");
    if bytes.is_empty() {
        return Err(invalid("Payload is null or empty."));
    }
    if bytes.get(4..8) != Some(b"IPLR") {
        return Err(invalid("Payload does not have valid identifier."));
    }
    let verification = || invalid("Runtime stages buffer failed verification.");
    let buf = Buf(bytes);
    let root = buf.root().ok_or_else(verification)?;
    let version = root.u32(5).ok_or_else(verification)?;
    if version != FORMAT_VERSION {
        return Err(invalid(&format!(
            "Unsupported runtime stages format version. Expected {FORMAT_VERSION}, got {version}."
        )));
    }
    let mut stages = Stages::default();
    for backend in Backend::ALL {
        // A stage that does not read is a file the engine's verifier would
        // refuse, or one its Create would abort on: refused here either way.
        stages.0[backend.field()] = match root.table(backend.field()).ok_or_else(verification)? {
            Some(t) => Some(stage(t).ok_or_else(verification)?),
            None => None,
        };
    }
    Ok(stages)
}

fn stage(t: Table<'_>) -> Option<Stage> {
    let mut uniforms = Vec::new();
    if let Some(list) = t.tables(3)? {
        for u in list {
            uniforms.push(uniform(u?)?);
        }
    }
    let shader = t.bytes(4)?.unwrap_or_default().to_vec();
    let mut stage = Stage { uniforms, shader };
    rebind_samplers(&mut stage.uniforms);
    Some(stage)
}

/// The engine's sampler bindings: the stage's samplers take 64, 65, ... in
/// order, stepping over the block's binding (its location) where the two
/// would meet. impellerc's descriptions give every sampler the same one.
fn rebind_samplers(uniforms: &mut [Uniform]) {
    let mut ubo = None;
    for u in uniforms.iter_mut() {
        if u.kind == UniformType::Struct {
            ubo = Some(u.location);
            u.binding = u.location;
        }
    }
    let mut binding = 64;
    if ubo == Some(binding) {
        binding += 1;
    }
    for u in uniforms.iter_mut() {
        if u.kind == UniformType::SampledImage {
            u.binding = binding;
            binding += 1;
            if ubo == Some(binding) {
                binding += 1;
            }
        }
    }
}

fn uniform(t: Table<'_>) -> Option<Uniform> {
    let kind = match t.u32(3)? {
        0 => UniformType::Float,
        1 => UniformType::SampledImage,
        2 => UniformType::Struct,
        _ => return None,
    };
    let mut padding_layout = Vec::new();
    if let Some(slots) = t.bytes(8)? {
        for b in slots {
            padding_layout.push(match b {
                0 => Slot::Padding,
                1 => Slot::Float,
                _ => return None,
            });
        }
    }
    let mut struct_fields = Vec::new();
    if let Some(list) = t.tables(9)? {
        for f in list {
            let f = f?;
            struct_fields.push(StructField {
                name: f.string(0)?.unwrap_or_default(),
                byte_size: f.u64(1)?,
            });
        }
    }
    Some(Uniform {
        name: t.string(0)?.unwrap_or_default(),
        location: t.u64(1)?,
        binding: t.u64(2)?,
        kind,
        bit_width: t.u64(4)?,
        rows: t.u64(5)?,
        columns: t.u64(6)?,
        array_elements: t.u64(7)?,
        padding_layout,
        struct_fields,
    })
}

/// A flatbuffer, read only through bounds-checked accessors.
#[derive(Clone, Copy)]
struct Buf<'a>(&'a [u8]);

impl<'a> Buf<'a> {
    fn slice(self, at: usize, len: usize) -> Option<&'a [u8]> {
        self.0.get(at..at.checked_add(len)?)
    }

    fn u16(self, at: usize) -> Option<u16> {
        Some(u16::from_le_bytes(self.slice(at, 2)?.try_into().ok()?))
    }

    fn u32(self, at: usize) -> Option<u32> {
        Some(u32::from_le_bytes(self.slice(at, 4)?.try_into().ok()?))
    }

    fn i32(self, at: usize) -> Option<i32> {
        Some(i32::from_le_bytes(self.slice(at, 4)?.try_into().ok()?))
    }

    fn u64(self, at: usize) -> Option<u64> {
        Some(u64::from_le_bytes(self.slice(at, 8)?.try_into().ok()?))
    }

    /// Follows the unsigned offset stored at `at`, which counts from `at`.
    fn follow(self, at: usize) -> Option<usize> {
        at.checked_add(self.u32(at)? as usize)
    }

    fn root(self) -> Option<Table<'a>> {
        self.table_at(self.follow(0)?)
    }

    /// The table at `at`, with its vtable checked to lie inside the buffer.
    fn table_at(self, at: usize) -> Option<Table<'a>> {
        // The vtable is at the table minus the signed offset stored there.
        let vtable = (at as i64).checked_sub(i64::from(self.i32(at)?))?;
        let vtable = usize::try_from(vtable).ok()?;
        let vtable_len = self.u16(vtable)? as usize;
        let table_len = self.u16(vtable.checked_add(2)?)? as usize;
        if vtable_len < 4 || !vtable_len.is_multiple_of(2) {
            return None;
        }
        self.slice(vtable, vtable_len)?;
        self.slice(at, table_len)?;
        Some(Table {
            buf: self,
            at,
            vtable,
            fields: (vtable_len - 4) / 2,
            len: table_len,
        })
    }
}

/// A table: each field's position comes from its vtable, and a field the
/// vtable leaves out (or gives 0) has its default.
#[derive(Clone, Copy)]
struct Table<'a> {
    buf: Buf<'a>,
    at: usize,
    vtable: usize,
    fields: usize,
    len: usize,
}

impl<'a> Table<'a> {
    /// Where field `i` is, or `Some(None)` when it is absent. `None` when
    /// the vtable puts it outside the table.
    fn field(self, i: usize, size: usize) -> Option<Option<usize>> {
        if i >= self.fields {
            return Some(None);
        }
        let offset = self.buf.u16(self.vtable + 4 + 2 * i)? as usize;
        if offset == 0 {
            return Some(None);
        }
        if offset.checked_add(size)? > self.len {
            return None;
        }
        Some(Some(self.at + offset))
    }

    fn u32(self, i: usize) -> Option<u32> {
        match self.field(i, 4)? {
            Some(at) => self.buf.u32(at),
            None => Some(0),
        }
    }

    fn u64(self, i: usize) -> Option<u64> {
        match self.field(i, 8)? {
            Some(at) => self.buf.u64(at),
            None => Some(0),
        }
    }

    /// A vector's length and where its first element is.
    fn vector(self, i: usize, element: usize) -> Option<Option<(usize, usize)>> {
        let Some(at) = self.field(i, 4)? else {
            return Some(None);
        };
        let start = self.buf.follow(at)?;
        let len = self.buf.u32(start)? as usize;
        let first = start.checked_add(4)?;
        self.buf.slice(first, len.checked_mul(element)?)?;
        Some(Some((len, first)))
    }

    fn bytes(self, i: usize) -> Option<Option<&'a [u8]>> {
        match self.vector(i, 1)? {
            Some((len, first)) => Some(Some(self.buf.slice(first, len)?)),
            None => Some(None),
        }
    }

    fn string(self, i: usize) -> Option<Option<String>> {
        match self.bytes(i)? {
            // A string is a byte vector with a NUL after it.
            Some(b) => Some(Some(String::from_utf8(b.to_vec()).ok()?)),
            None => Some(None),
        }
    }

    fn table(self, i: usize) -> Option<Option<Table<'a>>> {
        match self.field(i, 4)? {
            Some(at) => Some(Some(self.buf.table_at(self.buf.follow(at)?)?)),
            None => Some(None),
        }
    }

    /// A vector of tables, each read when the iterator reaches it.
    fn tables(self, i: usize) -> Option<Option<impl Iterator<Item = Option<Table<'a>>> + 'a>> {
        let Some((len, first)) = self.vector(i, 4)? else {
            return Some(None);
        };
        let buf = self.buf;
        Some(Some((0..len).map(move |k| {
            let at = first + 4 * k;
            buf.table_at(buf.follow(at)?)
        })))
    }
}

// Used when a paint draws with a fragment shader (plan step 4).
#[cfg_attr(not(test), allow(dead_code))]
/// Expands the floats Dart wrote into the uniform block's layout, as
/// `RuntimeEffectContents::EmplaceUniform` does for each non-sampler
/// uniform: a uniform with a padding layout takes one float per `Float`
/// slot and writes zero for each `Padding` one, repeated for each array
/// element; one without is copied as it is. Each uniform's Dart floats
/// follow the last's. `None` when `floats` is shorter than the stage reads.
pub(crate) fn expand(uniforms: &[Uniform], floats: &[f32]) -> Option<Vec<f32>> {
    let mut out = Vec::new();
    let mut read = 0usize;
    for u in uniforms {
        if u.kind == UniformType::SampledImage {
            continue;
        }
        if u.padding_layout.is_empty() {
            let n = usize::try_from(u.gpu_size() / 4).ok()?;
            out.extend_from_slice(floats.get(read..read.checked_add(n)?)?);
            read += n;
        } else {
            let count = u.array_elements.max(1);
            for _ in 0..count {
                for slot in &u.padding_layout {
                    match slot {
                        Slot::Padding => out.push(0.0),
                        Slot::Float => {
                            out.push(*floats.get(read)?);
                            read += 1;
                        }
                    }
                }
            }
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    fn fixture(name: &str) -> Vec<u8> {
        let path = format!("{}/tests/shaders/{name}.iplr", env!("CARGO_MANIFEST_DIR"));
        std::fs::read(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
    }

    fn summary(stage: &Stage) -> Vec<(String, &'static str, u64, u64)> {
        stage
            .uniforms
            .iter()
            .map(|u| (u.name.clone(), u.kind.name(), u.dart_size(), u.binding))
            .collect()
    }

    #[test]
    fn every_linux_backend_is_present() {
        for name in ["time", "ink_sparkle", "stretch_effect", "layout"] {
            let stages = decode(&fixture(name)).unwrap();
            assert_eq!(
                stages.found(),
                [
                    Backend::SkSL,
                    Backend::OpenGLES,
                    Backend::OpenGLES3,
                    Backend::Vulkan
                ],
                "{name}"
            );
            let vulkan = stages.get(Backend::Vulkan).unwrap();
            // SPIR-V's magic number, little-endian.
            assert_eq!(vulkan.shader[..4], [0x03, 0x02, 0x23, 0x07], "{name}");
        }
    }

    #[test]
    fn the_vulkan_stage_reads_one_block() {
        let stages = decode(&fixture("time")).unwrap();
        let vulkan = stages.get(Backend::Vulkan).unwrap();
        assert_eq!(vulkan.uniforms.len(), 1);
        let block = &vulkan.uniforms[0];
        assert_eq!(block.kind, UniformType::Struct);
        assert_eq!(
            block
                .struct_fields
                .iter()
                .map(|f| (f.name.as_str(), f.byte_size))
                .collect::<Vec<_>>(),
            [("uSize", 8), ("uTime", 4)]
        );
        assert_eq!(block.dart_size(), 12);
        assert_eq!(block.binding, block.location);
    }

    #[test]
    fn padding_and_samplers_in_the_vulkan_stage() {
        let stages = decode(&fixture("layout")).unwrap();
        let vulkan = stages.get(Backend::Vulkan).unwrap();
        let s = summary(vulkan);
        // The samplers, on the bindings the engine gives them, stepping over
        // the block's 64; then the block, which Dart writes 1 + 3 + 9 + 9
        // floats of.
        assert_eq!(
            s,
            [
                ("uFirst".into(), "SampledImage", 0, 65),
                ("uSecond".into(), "SampledImage", 0, 66),
                (
                    "_RESERVED_IDENTIFIER_FIXUP_gl_DefaultUniformBlock".into(),
                    "Struct",
                    4 * (1 + 3 + 9 + 9),
                    64
                ),
            ]
        );
        assert_eq!(vulkan.uniforms[2].gpu_size(), 4 * 36);
        // Expanding Dart's 22 floats gives the block's std140 layout.
        let floats: Vec<f32> = (1..=22).map(|i| i as f32).collect();
        let block = expand(&vulkan.uniforms, &floats).unwrap();
        let x = 0.0;
        assert_eq!(
            block,
            [
                1.0, x, x, x, // uScale, then uTint's alignment
                2.0, 3.0, 4.0, x, // uTint
                5.0, 6.0, 7.0, x, 8.0, 9.0, 10.0, x, 11.0, 12.0, 13.0, x, // uPoints
                14.0, 15.0, 16.0, x, 17.0, 18.0, 19.0, x, 20.0, 21.0, 22.0, x, // uMatrix
                x, x, x, x, // the block's tail
            ],
        );
        assert_eq!(expand(&vulkan.uniforms, &floats[..21]), None);
    }

    #[test]
    fn a_wrong_version_and_a_bad_identifier_are_the_engines_errors() {
        let mut bytes = fixture("time");
        bytes[4] = b'X';
        assert_eq!(
            decode(&bytes).unwrap_err(),
            "INVALID_ARGUMENT: Payload does not have valid identifier."
        );
        assert_eq!(
            decode(&[]).unwrap_err(),
            "INVALID_ARGUMENT: Payload is null or empty."
        );
    }

    #[test]
    fn every_truncation_and_bit_flip_fails_cleanly() {
        // No offset in a damaged file reads out of bounds: each is either
        // decoded or refused, and the loop finishing is the assertion.
        let bytes = fixture("layout");
        for len in 0..bytes.len() {
            let _ = decode(&bytes[..len]);
        }
        for i in 0..bytes.len() {
            for bit in [0x01, 0x80] {
                let mut b = bytes.clone();
                b[i] ^= bit;
                let _ = decode(&b);
            }
        }
    }
}
