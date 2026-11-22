use std::sync::{Arc, OnceLock};

use resvg::{tiny_skia, usvg};

const MAX_IMAGE_EDGE: u32 = 32_768;
const MAX_IMAGE_PIXELS: u32 = 32 * 1_024 * 1_024;
const SVG_NAMESPACE: &str = "http://www.w3.org/2000/svg";
pub const SANS_FAMILY: &str = "Geist";
pub const MONO_FAMILY: &str = "Geist Mono";
pub(super) const GEIST_REGULAR: &[u8] = include_bytes!("assets/fonts/Geist-Regular.ttf");
pub(super) const GEIST_SEMIBOLD: &[u8] = include_bytes!("assets/fonts/Geist-SemiBold.ttf");
pub(super) const GEIST_MONO: &[u8] = include_bytes!("assets/fonts/GeistMono-Regular.ttf");
const FONT_FILES: &[&[u8]] = &[GEIST_REGULAR, GEIST_SEMIBOLD, GEIST_MONO];

static BUNDLED_FONTS: OnceLock<Arc<usvg::fontdb::Database>> = OnceLock::new();
static SYSTEM_FONTS: OnceLock<Arc<usvg::fontdb::Database>> = OnceLock::new();

#[derive(Debug, thiserror::Error)]
pub enum RasterError {
    #[error("could not parse flow SVG ({0})")]
    Svg(#[from] resvg::usvg::Error),
    #[error("image scale must be finite and positive")]
    Scale,
    #[error("image exceeds the raster size limit")]
    Size,
    #[error("could not encode flow PNG ({0})")]
    Encode(String),
}

pub fn png(svg: &str, scale: f32) -> Result<Vec<u8>, RasterError> {
    if !scale.is_finite() || scale <= 0.0 {
        return Err(RasterError::Scale);
    }

    let (_, tree) = parse(svg)?;
    let dimensions = tree.size();
    let width = scaled_dimension(dimensions.width(), scale)?;
    let height = scaled_dimension(dimensions.height(), scale)?;
    if width * height > MAX_IMAGE_PIXELS {
        return Err(RasterError::Size);
    }

    let mut pixmap = tiny_skia::Pixmap::new(width, height).ok_or(RasterError::Size)?;
    let transform = tiny_skia::Transform::from_scale(scale, scale);
    resvg::render(&tree, transform, &mut pixmap.as_mut());
    pixmap
        .encode_png()
        .map_err(|error| RasterError::Encode(error.to_string()))
}

pub fn outlined_svg(svg: &str) -> Result<String, RasterError> {
    let (document, tree) = parse(svg)?;
    let mut outlined = tree.to_string(&usvg::WriteOptions::default());
    preserve_metadata(document.root_element(), &mut outlined);
    Ok(outlined)
}

fn scaled_dimension(value: f32, scale: f32) -> Result<u32, RasterError> {
    let dimension = (value * scale).ceil().max(1.0);
    if dimension > MAX_IMAGE_EDGE as f32 {
        return Err(RasterError::Size);
    }
    Ok(dimension as u32)
}

fn parse(svg: &str) -> Result<(usvg::roxmltree::Document<'_>, usvg::Tree), RasterError> {
    let document = usvg::roxmltree::Document::parse(svg).map_err(usvg::Error::from)?;
    let options = usvg::Options {
        fontdb: Arc::clone(bundled_fonts()),
        font_family: SANS_FAMILY.to_owned(),
        font_resolver: font_resolver(),
        image_href_resolver: usvg::ImageHrefResolver {
            resolve_data: Box::new(|_, _, _| None),
            resolve_string: Box::new(|_, _| None),
        },
        ..usvg::Options::default()
    };
    let tree = usvg::Tree::from_xmltree(&document, &options)?;
    Ok((document, tree))
}

fn bundled_fonts() -> &'static Arc<usvg::fontdb::Database> {
    BUNDLED_FONTS.get_or_init(|| {
        let mut fonts = usvg::fontdb::Database::new();
        for font in FONT_FILES {
            fonts.load_font_source(usvg::fontdb::Source::Binary(Arc::new(*font)));
        }
        fonts.set_sans_serif_family(SANS_FAMILY);
        fonts.set_monospace_family(MONO_FAMILY);
        Arc::new(fonts)
    })
}

fn font_resolver() -> usvg::FontResolver<'static> {
    let select_fallback = usvg::FontResolver::default_fallback_selector();
    usvg::FontResolver {
        select_font: usvg::FontResolver::default_font_selector(),
        select_fallback: Box::new(move |character, used, fonts| {
            let selected = select_fallback(character, used, fonts);
            if selected.is_some() || !Arc::ptr_eq(fonts, bundled_fonts()) {
                return selected;
            }
            let system_fonts = SYSTEM_FONTS.get_or_init(|| {
                let mut fonts = bundled_fonts().as_ref().clone();
                fonts.load_system_fonts();
                Arc::new(fonts)
            });
            *fonts = Arc::clone(system_fonts);
            select_fallback(character, used, fonts)
        }),
    }
}

fn preserve_metadata(root: usvg::roxmltree::Node<'_, '_>, outlined: &mut String) {
    use std::fmt::Write as _;

    let mut metadata = String::new();
    for tag in ["title", "desc"] {
        let value = root
            .children()
            .find(|node| node.has_tag_name((SVG_NAMESPACE, tag)))
            .and_then(|node| node.text());
        if let Some(value) = value {
            let escaped = value
                .replace('&', "&amp;")
                .replace('<', "&lt;")
                .replace('>', "&gt;");
            let _ = write!(metadata, "<{tag}>{escaped}</{tag}>");
        }
    }

    if metadata.is_empty() {
        return;
    }
    let Some(root_end) = outlined.find('>') else {
        return;
    };
    if outlined[..root_end].ends_with('/') {
        outlined.replace_range(
            root_end - 1..=root_end,
            &format!(" role=\"img\">{metadata}</svg>"),
        );
    } else {
        outlined.insert_str(root_end + 1, &metadata);
        outlined.insert_str(root_end, " role=\"img\"");
    }
}

#[cfg(test)]
mod tests {
    use resvg::tiny_skia::Pixmap;

    use super::{RasterError, outlined_svg, png};

    const TEXT_SAMPLE: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" width="96" height="48">
        <title>Payment &amp; confirmation</title>
        <desc>Reaches &lt;checkout&gt;.</desc>
        <text x="4" y="16" font-family="Geist" font-size="16">Café</text>
        <text x="4" y="32" font-family="Geist" font-size="16" font-weight="600">Ação</text>
        <text x="4" y="46" font-family="Geist Mono" font-size="13">Журнал</text>
    </svg>"#;

    #[test]
    fn third_party_notices_match_the_bundled_licenses() {
        let notices = include_str!("../../THIRD-PARTY-NOTICES.md");
        assert!(notices.contains(include_str!("assets/fonts/OFL.txt")));
        assert!(notices.contains(include_str!("assets/icons/LICENSE")));
    }

    #[test]
    fn png_uses_two_physical_pixels_per_logical_pixel() {
        let encoded = png(TEXT_SAMPLE, 2.0).unwrap();
        let image = Pixmap::decode_png(&encoded).unwrap();

        assert_eq!((image.width(), image.height()), (192, 96));
        assert!(
            image.pixels()[80 * 192..]
                .iter()
                .any(|pixel| pixel.alpha() != 0)
        );
    }

    #[test]
    fn png_draws_each_bundled_font() {
        let encoded = png(TEXT_SAMPLE, 1.0).unwrap();
        let image = Pixmap::decode_png(&encoded).unwrap();

        for line in image.pixels().chunks_exact(16 * 96) {
            assert!(line.iter().any(|pixel| pixel.alpha() != 0));
        }

        let regular_text = TEXT_SAMPLE.replace("font-weight=\"600\"", "font-weight=\"400\"");
        let regular = Pixmap::decode_png(&png(&regular_text, 1.0).unwrap()).unwrap();
        assert_ne!(image.pixels(), regular.pixels());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn png_uses_installed_fonts_for_cjk_text() {
        let characters = |text| {
            format!(
                r#"<svg xmlns="http://www.w3.org/2000/svg" width="96" height="40"><text x="4" y="30" font-family="Geist" font-size="22">{text}</text></svg>"#
            )
        };
        let checkout = Pixmap::decode_png(&png(&characters("結帳"), 1.0).unwrap()).unwrap();
        let shopping_cart = Pixmap::decode_png(&png(&characters("購物"), 1.0).unwrap()).unwrap();
        assert!(checkout.pixels() != shopping_cart.pixels());
    }

    #[test]
    fn export_svg_outlines_text_and_retains_accessible_metadata() {
        let outlined = outlined_svg(TEXT_SAMPLE).unwrap();
        let document = resvg::usvg::roxmltree::Document::parse(&outlined).unwrap();
        let root = document.root_element();

        assert!(root.descendants().any(|node| node.has_tag_name("path")));
        assert!(!root.descendants().any(|node| node.has_tag_name("text")));
        assert_eq!(root.attribute("role"), Some("img"));
        assert!(root.children().any(|node| {
            node.has_tag_name("title") && node.text() == Some("Payment & confirmation")
        }));
        assert!(root.children().any(|node| {
            node.has_tag_name("desc") && node.text() == Some("Reaches <checkout>.")
        }));
    }

    #[test]
    fn png_rejects_invalid_scales() {
        for scale in [0.0, -1.0, f32::NAN, f32::INFINITY] {
            assert!(matches!(png(TEXT_SAMPLE, scale), Err(RasterError::Scale)));
        }
    }

    #[test]
    fn png_rejects_large_dimensions_before_allocating() {
        let oversized = r#"<svg xmlns="http://www.w3.org/2000/svg" width="32000" height="32000"/>"#;
        assert!(matches!(png(oversized, 1.0), Err(RasterError::Size)));
    }

    #[test]
    fn malformed_svg_returns_a_parse_error() {
        assert!(matches!(outlined_svg("<svg"), Err(RasterError::Svg(_))));
    }
}
