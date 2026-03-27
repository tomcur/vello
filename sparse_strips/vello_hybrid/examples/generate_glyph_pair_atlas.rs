// Copyright 2025 the Vello Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Generate the lowercase Roboto glyph atlas used by the glyph-pair benchmark.

#[path = "support/glyph_pair_support.rs"]
mod glyph_pair_support;

use std::fs;

use glyph_pair_support::{GlyphPairData, atlas_output_path};
use vello_cpu::Pixmap;
use vello_cpu::RenderContext;
use vello_cpu::color::palette::css::BLACK;

fn main() {
    let output = parse_output_path();
    let pair_data = GlyphPairData::new();
    let glyphs = pair_data.atlas_glyphs();
    let layout = pair_data.layout;

    let mut ctx = RenderContext::new(layout.atlas_width, layout.atlas_height);
    ctx.set_paint(BLACK);
    ctx.glyph_run(&pair_data.font)
        .font_size(glyph_pair_support::FONT_SIZE)
        .hint(true)
        .fill_glyphs(glyphs.into_iter());
    ctx.flush();

    let mut pixmap = Pixmap::new(layout.atlas_width, layout.atlas_height);
    ctx.render_to_pixmap(&mut pixmap);

    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent).expect("failed to create atlas output directory");
    }
    fs::write(
        &output,
        pixmap.into_png().expect("failed to encode atlas PNG"),
    )
    .expect("failed to write atlas PNG");

    println!(
        "Wrote lowercase glyph atlas to {} ({}x{}, {} glyphs)",
        output.display(),
        layout.atlas_width,
        layout.atlas_height,
        glyph_pair_support::LETTER_COUNT,
    );
}

fn parse_output_path() -> std::path::PathBuf {
    let mut args = std::env::args().skip(1);
    let mut output = atlas_output_path();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--output" => {
                let path = args.next().expect("--output requires a path");
                output = path.into();
            }
            _ => panic!("unknown argument: {arg}"),
        }
    }
    output
}
