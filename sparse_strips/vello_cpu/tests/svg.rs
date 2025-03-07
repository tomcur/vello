use image::ImageEncoder;
use kurbo::Affine;
use vello_api::paint::Paint;
use vello_cpu::{Pixmap, RenderContext};

mod pico_svg;
use pico_svg::{Item, PicoSvg};

#[test]
fn foo() {
    const SCALE: f64 = 1.;
    let svg = PicoSvg::load(
        // include_str!("../../../examples/assets/Ghostscript_Tiger.svg"),
        include_str!("../../../../bintje/examples/assets/paris-30k/paris-30k.svg"),
        SCALE,
    )
    .unwrap();

    let width = (svg.size.width * SCALE).ceil();
    let height = (svg.size.height * SCALE).ceil();
    let width = 1920;
    let height = 1200;

    let mut pixmap = Pixmap::new(width as u16, height as u16);
    let mut render_ctx = RenderContext::new(width as u16, height as u16);

    const NUM_ITERATIONS: usize = 1;
    for iteration in 0..NUM_ITERATIONS {
        render_ctx.reset();
        encode_svg(&mut render_ctx, 1. / SCALE, Affine::IDENTITY, &svg.items);
        render_ctx.render_to_pixmap(&mut pixmap);
    }
    println!(
        "Tile generation elapsed:  {:?}ms",
        render_ctx.tile_generation_elapsed.as_nanos() as f32 / (NUM_ITERATIONS as f32 * 1_000_000.)
    );
    println!(
        "Tile sorting elapsed:  {:?}ms",
        render_ctx.tile_sorting_elapsed.as_nanos() as f32 / (NUM_ITERATIONS as f32 * 1_000_000.)
    );
    println!(
        "Strip generation elapsed: {:?}ms",
        render_ctx.strip_generation_elapsed.as_nanos() as f32
            / (NUM_ITERATIONS as f32 * 1_000_000.)
    );

    pixmap.unpremultiply();
    let file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open("test.png")
        .unwrap();
    let encoder = image::codecs::png::PngEncoder::new(file);
    encoder
        .write_image(
            pixmap.data(),
            width as u32,
            height as u32,
            image::ExtendedColorType::Rgba8,
        )
        .unwrap();
}

fn encode_svg(render_ctx: &mut RenderContext, scale_recip: f64, transform: Affine, items: &[Item]) {
    render_ctx.set_transform(transform);
    for item in items {
        match item {
            Item::Fill(fill) => {
                render_ctx.set_paint(Paint::Solid(fill.color));
                render_ctx.fill_path(&fill.path);
            }
            Item::Stroke(stroke) => {
                render_ctx.set_paint(Paint::Solid(stroke.color));
                render_ctx.set_stroke(kurbo::Stroke {
                    width: stroke.width * scale_recip,
                    ..kurbo::Stroke::default()
                });
                render_ctx.stroke_path(&stroke.path);
            }
            Item::Group(group) => {
                encode_svg(
                    render_ctx,
                    scale_recip,
                    transform * group.affine,
                    &group.children,
                );
                render_ctx.set_transform(transform);
            }
        }
    }
}
