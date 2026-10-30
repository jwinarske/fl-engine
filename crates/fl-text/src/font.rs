//! The font collection, as txt's `FontCollection` orders its managers
//! (font_collection.cc `GetFontManagerOrder`): fonts loaded at run time
//! (`loadFontFromList`), then the bundle's (`FontManifest.json`), then the
//! system's (D4, through fontdb). A family is matched by name, ignoring
//! case, and a face within it by SkFontStyleSet's CSS3 rules.
//!
//! Bundle fonts are registered by name and read on first use; the system
//! is scanned on the first miss, once (brief M3 §15). System font files
//! are read into memory, never mapped.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use harfrust::Font;

use crate::style::FontStyle;

/// One face of a font file: its bytes and what it is.
pub struct Face {
    /// Unique in the process.
    pub id: u64,
    pub family: String,
    pub style: FontStyle,
    /// The face at its default location.
    pub(crate) font: Font,
    pub(crate) data: Arc<Vec<u8>>,
    pub(crate) index: u32,
}

impl std::fmt::Debug for Face {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Face({} #{} {:?})", self.family, self.id, self.style)
    }
}

impl Face {
    /// The face at `index` in `data`, named `family`, or its own name if
    /// that is empty. `None` if the data is no font.
    /// Parsed under `catch_unwind`: a font that panics the parser is no
    /// font (brief M3 §15).
    pub fn new(data: Arc<Vec<u8>>, index: u32, family: &str) -> Option<Arc<Face>> {
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            Face::parse(data, index, family)
        }))
        .ok()
        .flatten()
    }

    fn parse(data: Arc<Vec<u8>>, index: u32, family: &str) -> Option<Arc<Face>> {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        // The tables every face needs, as FreeType needs them before Skia
        // has a typeface: a table directory alone is no font.
        {
            use skrifa::raw::TableProvider;
            let f = skrifa::FontRef::from_index(data.as_slice(), index).ok()?;
            if f.head().is_err() || f.maxp().is_err() || f.cmap().is_err() {
                return None;
            }
        }
        let blob: Arc<dyn AsRef<[u8]> + Send + Sync> = data.clone();
        let font = Font::new(blob, index)?;
        let attrs = font.default_attributes();
        let style = FontStyle {
            weight: attrs.weight.value().round().clamp(1.0, 1000.0) as u16,
            italic: !matches!(attrs.style, read_style_normal!()),
        };
        let family = if family.is_empty() {
            family_name(&font).unwrap_or_default()
        } else {
            family.to_owned()
        };
        Some(Arc::new(Face {
            id: NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            family,
            style,
            font,
            data,
            index,
        }))
    }

    /// The glyph for `c`, if the face has one.
    pub(crate) fn has_char(&self, c: char) -> bool {
        self.font.charmap().map_unicode(c as u32).is_some()
    }

    /// The bytes and the index within them, for a rasterizer.
    pub fn data(&self) -> (&Arc<Vec<u8>>, u32) {
        (&self.data, self.index)
    }
}

/// `Style::Normal`'s pattern, whichever path it is exported at.
macro_rules! read_style_normal {
    () => {
        harfrust::font::attribute::Style::Normal
    };
}
use read_style_normal;

/// The face's family name from its `name` table: the typographic family if
/// it has one, else the legacy family.
fn family_name(font: &Font) -> Option<String> {
    use skrifa::raw::types::NameId;
    let names = font.names();
    [NameId::TYPOGRAPHIC_FAMILY_NAME, NameId::FAMILY_NAME]
        .into_iter()
        .find_map(|id| names.get(id))
        .and_then(|n| n.english_or_first())
        .map(|l| l.name().chars().collect())
}

/// A face at a location in its design space: what a run is shaped and
/// drawn with.
#[derive(Clone)]
pub struct Typeface {
    pub face: Arc<Face>,
    /// `(tag, value)`, the user-space coordinates set.
    pub variations: Arc<Vec<([u8; 4], f32)>>,
    pub(crate) font: Font,
}

impl std::fmt::Debug for Typeface {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Typeface({:?} {:?})", self.face, self.variations)
    }
}

impl PartialEq for Typeface {
    fn eq(&self, other: &Self) -> bool {
        self.face.id == other.face.id && self.variations == other.variations
    }
}

impl Typeface {
    /// `face` at `variations`; axes it lacks are ignored.
    pub(crate) fn new(face: &Arc<Face>, variations: &[([u8; 4], f32)]) -> Typeface {
        let has_axes = !face.font.axes().is_empty();
        let font = if has_axes && !variations.is_empty() {
            face.font
                .instance_builder()
                .variations(variations.iter().map(|(t, v)| (harfrust::Tag::new(t), *v)))
                .build()
        } else {
            face.font.clone()
        };
        Typeface {
            face: Arc::clone(face),
            variations: Arc::new(if has_axes {
                variations.to_vec()
            } else {
                Vec::new()
            }),
            font,
        }
    }
}

/// SkFontStyleSet::matchStyleCSS3, width always normal: the asked slant if
/// any face has it, then the weight by CSS's rule. Asked 400 to 500: that
/// weight, heavier up to 500, lighter, then heavier past 500. Asked below
/// 400: lighter, then heavier. Above 500: heavier, then lighter. The index
/// of the face.
pub(crate) fn match_style_index(styles: &[FontStyle], want: FontStyle) -> Option<usize> {
    let slant_ok = styles.iter().any(|s| s.italic == want.italic);
    let w = i32::from(want.weight);
    let key = |s: &FontStyle| -> (i32, i32, i32) {
        let slant = i32::from(slant_ok && s.italic != want.italic);
        let fw = i32::from(s.weight);
        let (group, distance) = if fw == w {
            (0, 0)
        } else if (400..=500).contains(&w) {
            if fw > w && fw <= 500 {
                (1, fw - w)
            } else if fw < w {
                (2, w - fw)
            } else {
                (3, fw - w)
            }
        } else if w < 400 {
            if fw < w {
                (1, w - fw)
            } else {
                (2, fw - w)
            }
        } else if fw > w {
            (1, fw - w)
        } else {
            (2, w - fw)
        };
        (slant, group, distance)
    };
    (0..styles.len()).min_by_key(|i| key(&styles[*i]))
}

/// The face of `faces` [`match_style_index`] picks.
pub(crate) fn match_style(faces: &[Arc<Face>], want: FontStyle) -> Option<&Arc<Face>> {
    let styles: Vec<FontStyle> = faces.iter().map(|f| f.style).collect();
    match_style_index(&styles, want).map(|i| &faces[i])
}

/// A family whose faces load on first use: a bundle's.
struct LazyFamily {
    name: String,
    keys: Vec<String>,
    faces: OnceLock<Vec<Arc<Face>>>,
}

/// Reads a bundle asset's bytes by key.
pub type AssetLoader = Arc<dyn Fn(&str) -> Option<Vec<u8>> + Send + Sync>;

/// Fallback faces by code point and style (brief M3 §15).
type FallbackCache = HashMap<(char, FontStyle), Option<Arc<Face>>>;

/// Matched faces by families and style.
type FaceCache = HashMap<(Vec<String>, FontStyle), Vec<Arc<Face>>>;

/// The system's fonts, scanned once.
struct System {
    db: fontdb::Database,
    /// Faces loaded from the database, by its id.
    loaded: Mutex<HashMap<fontdb::ID, Option<Arc<Face>>>>,
    /// Fallback answers by code point and style.
    fallback: Mutex<FallbackCache>,
}

/// The default families on Linux, in order (platform_linux.cc).
pub const DEFAULT_FAMILIES: &[&str] = &[
    "Ubuntu",
    "Adwaita Sans",
    "Cantarell",
    "DejaVu Sans",
    "Liberation Sans",
    "Arial",
];

/// Every font text can be set in.
#[derive(Default)]
pub struct FontCollection {
    dynamic: Mutex<Vec<Arc<Face>>>,
    assets: Mutex<Vec<Arc<LazyFamily>>>,
    loader: Mutex<Option<AssetLoader>>,
    system: OnceLock<System>,
    /// Whether to look in the system's fonts at all.
    use_system: bool,
    /// Matched faces by families and style: SkParagraph's face cache.
    cache: Mutex<FaceCache>,
}

impl std::fmt::Debug for FontCollection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "FontCollection")
    }
}

fn lower(s: &str) -> String {
    s.to_lowercase()
}

impl FontCollection {
    /// A collection that falls back to the system's fonts.
    pub fn new() -> FontCollection {
        FontCollection {
            use_system: true,
            ..FontCollection::default()
        }
    }

    /// One that never reads the system's fonts: for tests, and for goldens,
    /// which name every font they draw.
    pub fn without_system_fonts() -> FontCollection {
        FontCollection::default()
    }

    /// `FontCollection.loadFontFromList`: `data` as `family`, or as its own
    /// family name if that is empty. False if it is not a font.
    pub fn add_font(&self, data: Vec<u8>, family: &str) -> bool {
        let data = Arc::new(data);
        let Some(face) = Face::new(data, 0, family) else {
            return false;
        };
        self.dynamic
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(face);
        self.cache.lock().unwrap_or_else(|e| e.into_inner()).clear();
        true
    }

    /// The bundle's families, `FontManifest.json`'s, read through `loader`
    /// when first matched.
    pub fn set_assets(&self, families: Vec<(String, Vec<String>)>, loader: AssetLoader) {
        *self.assets.lock().unwrap_or_else(|e| e.into_inner()) = families
            .into_iter()
            .map(|(name, keys)| {
                Arc::new(LazyFamily {
                    name,
                    keys,
                    faces: OnceLock::new(),
                })
            })
            .collect();
        *self.loader.lock().unwrap_or_else(|e| e.into_inner()) = Some(loader);
        self.cache.lock().unwrap_or_else(|e| e.into_inner()).clear();
    }

    /// The faces of the family named `name`, from the first manager that
    /// has it.
    fn family(&self, name: &str) -> Vec<Arc<Face>> {
        let key = lower(name);
        let dynamic: Vec<Arc<Face>> = self
            .dynamic
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .filter(|f| lower(&f.family) == key)
            .cloned()
            .collect();
        if !dynamic.is_empty() {
            return dynamic;
        }
        let assets: Vec<Arc<LazyFamily>> = self
            .assets
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .filter(|f| lower(&f.name) == key)
            .cloned()
            .collect();
        for fam in assets {
            let faces = fam.faces.get_or_init(|| {
                let loader = self
                    .loader
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .clone();
                fam.keys
                    .iter()
                    .filter_map(|k| loader.as_ref().and_then(|l| l(k)))
                    // A malformed font is that font missing.
                    .filter_map(|d| Face::new(Arc::new(d), 0, &fam.name))
                    .collect()
            });
            if !faces.is_empty() {
                return faces.clone();
            }
        }
        self.system_family(&key)
    }

    fn system(&self) -> Option<&System> {
        if !self.use_system {
            return None;
        }
        Some(self.system.get_or_init(|| {
            let mut db = fontdb::Database::new();
            db.load_system_fonts();
            System {
                db,
                loaded: Mutex::new(HashMap::new()),
                fallback: Mutex::new(HashMap::new()),
            }
        }))
    }

    /// A system face, read into memory on first use.
    fn system_face(sys: &System, id: fontdb::ID) -> Option<Arc<Face>> {
        if let Some(f) = sys
            .loaded
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&id)
        {
            return f.clone();
        }
        let info = sys.db.face(id)?;
        let family = info
            .families
            .first()
            .map(|(n, _)| n.clone())
            .unwrap_or_default();
        let face = sys
            .db
            .with_face_data(id, |data, index| (data.to_vec(), index))
            .and_then(|(v, index)| Face::new(Arc::new(v), index, &family));
        sys.loaded
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(id, face.clone());
        face
    }

    fn system_family(&self, key: &str) -> Vec<Arc<Face>> {
        let Some(sys) = self.system() else {
            return Vec::new();
        };
        let ids: Vec<fontdb::ID> = sys
            .db
            .faces()
            .filter(|f| f.families.iter().any(|(n, _)| lower(n) == key))
            .map(|f| f.id)
            .collect();
        ids.into_iter()
            .filter_map(|id| Self::system_face(sys, id))
            .collect()
    }

    /// `findTypefaces`: a face for each family that has one, in order; if
    /// none does, the first default family's, or the system's default.
    pub fn find(&self, families: &[String], style: FontStyle) -> Vec<Arc<Face>> {
        let key = (families.to_vec(), style);
        if let Some(found) = self
            .cache
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&key)
        {
            return found.clone();
        }
        let mut out: Vec<Arc<Face>> = families
            .iter()
            .filter(|f| !f.is_empty())
            .filter_map(|f| match_style(&self.family(f), style).cloned())
            .collect();
        if out.is_empty() {
            let default = DEFAULT_FAMILIES
                .iter()
                .find_map(|f| match_style(&self.family(f), style).cloned())
                .or_else(|| self.system_default(style));
            out.extend(default);
        }
        self.cache
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(key, out.clone());
        out
    }

    /// `legacyMakeTypeface(nullptr, style)`: any sans face.
    fn system_default(&self, style: FontStyle) -> Option<Arc<Face>> {
        let sys = self.system()?;
        let query = fontdb::Query {
            families: &[fontdb::Family::SansSerif],
            weight: fontdb::Weight(style.weight),
            stretch: fontdb::Stretch::Normal,
            style: if style.italic {
                fontdb::Style::Italic
            } else {
                fontdb::Style::Normal
            },
        };
        let id = sys
            .db
            .query(&query)
            .or_else(|| sys.db.faces().next().map(|f| f.id))?;
        Self::system_face(sys, id)
    }

    /// `defaultFallback`: a system face with a glyph for `c`, the closest
    /// to `style`. The bundle's and loaded fonts are never fallbacks, as
    /// the engine's asset managers match no characters.
    pub fn fallback(&self, c: char, style: FontStyle) -> Option<Arc<Face>> {
        let sys = self.system()?;
        if let Some(f) = sys
            .fallback
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&(c, style))
        {
            return f.clone();
        }
        // The default families first, as fontconfig's configuration would
        // put them, then every family in database order.
        let mut names: Vec<String> = DEFAULT_FAMILIES.iter().map(|s| lower(s)).collect();
        for info in sys.db.faces() {
            if let Some((n, _)) = info.families.first() {
                let n = lower(n);
                if !names.contains(&n) {
                    names.push(n);
                }
            }
        }
        let found = names.iter().find_map(|n| {
            let faces: Vec<Arc<Face>> = self
                .system_family(n)
                .into_iter()
                .filter(|f| f.has_char(c))
                .collect();
            match_style(&faces, style).cloned()
        });
        sys.fallback
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert((c, style), found.clone());
        found
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn styles(ws: &[(u16, bool)]) -> Vec<FontStyle> {
        ws.iter()
            .map(|(weight, italic)| FontStyle {
                weight: *weight,
                italic: *italic,
            })
            .collect()
    }

    #[test]
    fn data_that_is_no_font_is_none() {
        for data in [
            vec![],
            vec![0u8; 12],
            b"OTTO\0\x01\0\0garbage".to_vec(),
            vec![0xFF; 4096],
        ] {
            assert!(Face::new(Arc::new(data), 0, "x").is_none());
        }
        let c = FontCollection::without_system_fonts();
        assert!(!c.add_font(vec![1, 2, 3], "x"));
    }

    #[test]
    fn css_weight_matching() {
        let s = styles(&[(300, false), (500, false), (700, false)]);
        let w = |weight| {
            match_style_index(
                &s,
                FontStyle {
                    weight,
                    italic: false,
                },
            )
            .map(|i| s[i].weight)
        };
        assert_eq!(w(400), Some(500), "400 looks up to 500 first");
        assert_eq!(w(350), Some(300), "below 400 looks lighter first");
        assert_eq!(w(600), Some(700), "above 500 looks heavier first");
        assert_eq!(w(800), Some(700));
        assert_eq!(w(700), Some(700));
        let s = styles(&[(300, false), (700, false)]);
        let w = |weight| {
            match_style_index(
                &s,
                FontStyle {
                    weight,
                    italic: false,
                },
            )
            .map(|i| s[i].weight)
        };
        assert_eq!(w(400), Some(300), "then lighter, before heavier past 500");
    }

    #[test]
    fn slant_before_weight() {
        let s = styles(&[(400, false), (700, true)]);
        let i = match_style_index(
            &s,
            FontStyle {
                weight: 400,
                italic: true,
            },
        );
        assert_eq!(i, Some(1));
        // No italic face: the weight decides among the upright ones.
        let s = styles(&[(300, false), (400, false)]);
        assert_eq!(
            match_style_index(
                &s,
                FontStyle {
                    weight: 400,
                    italic: true
                }
            ),
            Some(1)
        );
    }
}
