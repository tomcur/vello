// Copyright 2025 the Vello Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Shared support for the lowercase glyph atlas generator and benchmark examples.

#![allow(dead_code)]

use std::path::PathBuf;
use std::sync::Arc;

use skrifa::{
    MetadataProvider,
    raw::{FileRef, FontRef},
};
use glifo::Glyph;
use vello_common::geometry::RectU16;
use vello_common::kurbo::Affine;
use vello_common::peniko::{Blob, FontData};
use vello_hybrid::SampleRect;

const ROBOTO_FONT: &[u8] = include_bytes!("../../../../examples/assets/roboto/Roboto-Regular.ttf");

pub(crate) const LETTER_COUNT: usize = 26;
pub(crate) const PAIR_COUNT: usize = LETTER_COUNT * LETTER_COUNT;
pub(crate) const TRIPLE_COUNT: usize = LETTER_COUNT * LETTER_COUNT * LETTER_COUNT;
pub(crate) const FONT_SIZE: f32 = 24.0;
pub(crate) const CELL_PADDING_X: f32 = 4.0;
pub(crate) const CELL_PADDING_Y: f32 = 4.0;
pub(crate) const DEFAULT_ITERATIONS: u32 = 100;
pub(crate) const DEFAULT_WARMUP: u32 = 10;
const MAX_SCENE_DIMENSION: u16 = 8192;

#[derive(Clone, Copy, Debug)]
pub(crate) struct GlyphAtlasLayout {
    pub(crate) glyph_cell_width: u16,
    pub(crate) pair_cell_width: u16,
    pub(crate) triple_cell_width: u16,
    pub(crate) cell_height: u16,
    pub(crate) baseline: f32,
    pub(crate) atlas_width: u16,
    pub(crate) atlas_height: u16,
    pub(crate) block_cols: u16,
    pub(crate) block_rows: u16,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct SceneDimensions {
    pub(crate) width: u16,
    pub(crate) height: u16,
}

#[derive(Clone, Debug)]
pub(crate) struct GlyphPairData {
    pub(crate) font: FontData,
    pub(crate) layout: GlyphAtlasLayout,
    glyph_ids: [u32; LETTER_COUNT],
    advances: [f32; LETTER_COUNT],
    /// Per-glyph tight source rects within the atlas image (pixel coords).
    atlas_ink_rects: [RectU16; LETTER_COUNT],
    /// Per-glyph ink offset from glyph origin to ink top-left (in scene/image coords).
    ink_offsets: [(f64, f64); LETTER_COUNT],
}

impl GlyphPairData {
    pub(crate) fn new() -> Self {
        let font = FontData::new(Blob::new(Arc::new(ROBOTO_FONT)), 0);
        let font_ref = font_ref(&font);
        let font_size = skrifa::instance::Size::new(FONT_SIZE);
        let axes = font_ref.axes();
        let variations: [(&str, f32); 0] = [];
        let var_loc = axes.location(variations);
        let glyph_metrics = font_ref.glyph_metrics(font_size, &var_loc);
        let metrics = font_ref.metrics(font_size, &var_loc);
        let charmap = font_ref.charmap();

        let mut glyph_ids = [0; LETTER_COUNT];
        let mut advances = [0.0; LETTER_COUNT];
        let mut ink_bounds_raw = [[0.0f32; 4]; LETTER_COUNT]; // [x_min, y_min, x_max, y_max]
        for (index, ch) in ('a'..='z').enumerate() {
            let glyph_id = charmap.map(ch).unwrap_or_default();
            glyph_ids[index] = glyph_id.to_u32();
            advances[index] = glyph_metrics.advance_width(glyph_id).unwrap_or_default();
            if let Some(bbox) = glyph_metrics.bounds(glyph_id) {
                ink_bounds_raw[index] = [bbox.x_min, bbox.y_min, bbox.x_max, bbox.y_max];
            }
        }

        let max_glyph_advance = advances.iter().copied().fold(0.0, f32::max);
        let mut max_triple_advance: f32 = 0.0;
        for first in advances {
            for second in advances {
                for third in advances {
                    max_triple_advance = max_triple_advance.max(first + second + third);
                }
            }
        }

        let glyph_cell_width = (max_glyph_advance + 2.0 * CELL_PADDING_X).ceil() as u16;
        let pair_cell_width = (2.0 * max_glyph_advance + 2.0 * CELL_PADDING_X).ceil() as u16;
        let triple_cell_width = (max_triple_advance + 2.0 * CELL_PADDING_X).ceil() as u16;
        let cell_height =
            (metrics.ascent - metrics.descent + metrics.leading + 2.0 * CELL_PADDING_Y).ceil()
                as u16;
        let baseline = CELL_PADDING_Y + metrics.ascent;
        let atlas_width = glyph_cell_width * LETTER_COUNT as u16;
        let atlas_height = cell_height;
        let block_width = triple_cell_width * LETTER_COUNT as u16;
        let block_height = cell_height * LETTER_COUNT as u16;
        let (block_cols, block_rows) = choose_block_layout(block_width, block_height);

        // Compute per-glyph tight atlas source rects and ink offsets.
        let mut atlas_ink_rects = [RectU16::ZERO; LETTER_COUNT];
        let mut ink_offsets = [(0.0, 0.0); LETTER_COUNT];
        for index in 0..LETTER_COUNT {
            let [x_min, y_min, x_max, y_max] = ink_bounds_raw[index];
            let ix_min = f64::from(x_min.floor());
            let iy_max = f64::from(y_max.ceil());
            let ix_max = f64::from(x_max.ceil());
            let iy_min = f64::from(y_min.floor());

            let glyph_atlas_x =
                f64::from(index as u16 * glyph_cell_width) + f64::from(CELL_PADDING_X);
            let atlas_baseline = f64::from(baseline);

            let src_y0 = (atlas_baseline - iy_max).floor();
            let src_y1 = (atlas_baseline - iy_min).ceil();

            atlas_ink_rects[index] = RectU16::new(
                (glyph_atlas_x + ix_min) as u16,
                src_y0 as u16,
                (glyph_atlas_x + ix_max) as u16,
                src_y1 as u16,
            );
            // Offset from glyph origin to ink top-left in scene coordinates (y-down).
            // `src_y0 - atlas_baseline` ensures that `cell_y + baseline + dy` is integer.
            ink_offsets[index] = (ix_min, src_y0 - atlas_baseline);
        }

        Self {
            font,
            layout: GlyphAtlasLayout {
                glyph_cell_width,
                pair_cell_width,
                triple_cell_width,
                cell_height,
                baseline,
                atlas_width,
                atlas_height,
                block_cols,
                block_rows,
            },
            glyph_ids,
            advances,
            atlas_ink_rects,
            ink_offsets,
        }
    }

    pub(crate) fn atlas_glyphs(&self) -> Vec<Glyph> {
        let mut glyphs = Vec::with_capacity(LETTER_COUNT);
        for letter_index in 0..LETTER_COUNT {
            glyphs.push(Glyph {
                id: self.glyph_ids[letter_index],
                x: letter_index as f32 * f32::from(self.layout.glyph_cell_width) + CELL_PADDING_X,
                y: self.layout.baseline,
            });
        }
        glyphs
    }

    pub(crate) fn scene_glyphs(&self) -> Vec<Glyph> {
        let placements = self.triple_placements();
        let mut glyphs = Vec::with_capacity(placements.len() * 3);
        for (triple_index, cell_x, cell_y) in placements {
            self.push_triple_glyphs(triple_index, cell_x, cell_y, &mut glyphs);
        }
        glyphs
    }

    pub(crate) fn texture_rects(&self) -> Vec<SampleRect> {
        let placements = self.triple_placements();
        let mut rects = Vec::with_capacity(placements.len() * 3);
        let baseline = f64::from(self.layout.baseline);
        for (triple_index, cell_x, cell_y) in placements {
            let (first_index, second_index, third_index) = triple_letter_indices(triple_index);
            let glyph_y = cell_y + baseline;
            let padding_x = f64::from(CELL_PADDING_X);

            let (dx1, dy1) = self.ink_offsets[first_index];
            rects.push(SampleRect {
                source_region: self.atlas_ink_rects[first_index],
                transform: Affine::translate((cell_x + padding_x + dx1, glyph_y + dy1)),
            });

            let (dx2, dy2) = self.ink_offsets[second_index];
            rects.push(SampleRect {
                source_region: self.atlas_ink_rects[second_index],
                transform: Affine::translate((
                    cell_x + padding_x + f64::from(self.advances[first_index]).round() + dx2,
                    glyph_y + dy2,
                )),
            });

            let (dx3, dy3) = self.ink_offsets[third_index];
            rects.push(SampleRect {
                source_region: self.atlas_ink_rects[third_index],
                transform: Affine::translate((
                    cell_x
                        + padding_x
                        + (f64::from(self.advances[first_index])
                            + f64::from(self.advances[second_index]))
                        .round()
                        + dx3,
                    glyph_y + dy3,
                )),
            });
        }
        rects
    }

    pub(crate) fn scene_dimensions(&self) -> SceneDimensions {
        SceneDimensions {
            width: self.triple_grid_width() * self.layout.block_cols,
            height: self.triple_grid_height() * self.layout.block_rows,
        }
    }

    pub(crate) fn triple_grid_width(&self) -> u16 {
        self.layout.triple_cell_width * LETTER_COUNT as u16
    }

    pub(crate) fn triple_grid_height(&self) -> u16 {
        self.layout.cell_height * LETTER_COUNT as u16
    }

    pub(crate) fn pair_scene_dimensions(&self) -> SceneDimensions {
        SceneDimensions {
            width: self.layout.pair_cell_width * LETTER_COUNT as u16,
            height: self.layout.cell_height * LETTER_COUNT as u16,
        }
    }

    pub(crate) fn pair_glyphs(&self) -> Vec<Glyph> {
        let mut glyphs = Vec::with_capacity(PAIR_COUNT * 2);
        for first in 0..LETTER_COUNT {
            for second in 0..LETTER_COUNT {
                let cell_x = second as f32 * f32::from(self.layout.pair_cell_width);
                let cell_y = first as f32 * f32::from(self.layout.cell_height);
                let x = cell_x + CELL_PADDING_X;
                let y = cell_y + self.layout.baseline;
                glyphs.push(Glyph {
                    id: self.glyph_ids[first],
                    x,
                    y,
                });
                glyphs.push(Glyph {
                    id: self.glyph_ids[second],
                    x: x + self.advances[first],
                    y,
                });
            }
        }
        glyphs
    }

    pub(crate) fn pair_texture_rects(&self) -> Vec<SampleRect> {
        let mut rects = Vec::with_capacity(PAIR_COUNT * 2);
        let baseline = f64::from(self.layout.baseline);
        let padding_x = f64::from(CELL_PADDING_X);
        for first in 0..LETTER_COUNT {
            for second in 0..LETTER_COUNT {
                let cell_x = f64::from(second as u16 * self.layout.pair_cell_width);
                let cell_y = f64::from(first as u16 * self.layout.cell_height);
                let glyph_y = cell_y + baseline;

                let (dx1, dy1) = self.ink_offsets[first];
                rects.push(SampleRect {
                    source_region: self.atlas_ink_rects[first],
                    transform: Affine::translate((cell_x + padding_x + dx1, glyph_y + dy1)),
                });

                let (dx2, dy2) = self.ink_offsets[second];
                rects.push(SampleRect {
                    source_region: self.atlas_ink_rects[second],
                    transform: Affine::translate((
                        cell_x + padding_x + f64::from(self.advances[first]).round() + dx2,
                        glyph_y + dy2,
                    )),
                });
            }
        }
        rects
    }

    fn triple_placements(&self) -> Vec<(usize, f64, f64)> {
        let mut placements = Vec::with_capacity(TRIPLE_COUNT);
        let block_width = f64::from(self.triple_grid_width());
        let block_height = f64::from(self.triple_grid_height());
        let block_cols = usize::from(self.layout.block_cols);
        for block_index in 0..LETTER_COUNT {
            let block_col = block_index % block_cols;
            let block_row = block_index / block_cols;
            let block_x = block_col as f64 * block_width;
            let block_y = block_row as f64 * block_height;
            for inner_index in 0..(LETTER_COUNT * LETTER_COUNT) {
                let (col, row) = pair_grid_position(inner_index);
                let triple_index = block_index * LETTER_COUNT * LETTER_COUNT + inner_index;
                let x = block_x + f64::from(col as u16 * self.layout.triple_cell_width);
                let y = block_y + f64::from(row as u16 * self.layout.cell_height);
                placements.push((triple_index, x, y));
            }
        }
        placements
    }

    fn push_triple_glyphs(
        &self,
        triple_index: usize,
        cell_x: f64,
        cell_y: f64,
        glyphs: &mut Vec<Glyph>,
    ) {
        let (first_index, second_index, third_index) = triple_letter_indices(triple_index);
        let first_x = cell_x as f32 + CELL_PADDING_X;
        let y = cell_y as f32 + self.layout.baseline;
        glyphs.push(Glyph {
            id: self.glyph_ids[first_index],
            x: first_x,
            y,
        });
        glyphs.push(Glyph {
            id: self.glyph_ids[second_index],
            x: first_x + self.advances[first_index],
            y,
        });
        glyphs.push(Glyph {
            id: self.glyph_ids[third_index],
            x: first_x + self.advances[first_index] + self.advances[second_index],
            y,
        });
    }
}

pub(crate) fn atlas_output_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples/assets/glyphs_roboto_lowercase.png")
}

#[cfg(test)]
fn triple_name(triple_index: usize) -> String {
    let (first, second, third) = triple_letter_indices(triple_index);
    format!(
        "{}{}{}",
        (b'a' + first as u8) as char,
        (b'a' + second as u8) as char,
        (b'a' + third as u8) as char
    )
}

fn triple_letter_indices(triple_index: usize) -> (usize, usize, usize) {
    let first = triple_index / (LETTER_COUNT * LETTER_COUNT);
    let rem = triple_index % (LETTER_COUNT * LETTER_COUNT);
    let second = rem / LETTER_COUNT;
    let third = rem % LETTER_COUNT;
    (first, second, third)
}

fn pair_grid_position(pair_index: usize) -> (usize, usize) {
    (pair_index % LETTER_COUNT, pair_index / LETTER_COUNT)
}

fn choose_block_layout(block_width: u16, block_height: u16) -> (u16, u16) {
    let mut best = None;
    for cols in 1..=LETTER_COUNT as u16 {
        let rows = (LETTER_COUNT as u16).div_ceil(cols);
        let width = u32::from(block_width) * u32::from(cols);
        let height = u32::from(block_height) * u32::from(rows);
        if width <= u32::from(MAX_SCENE_DIMENSION) && height <= u32::from(MAX_SCENE_DIMENSION) {
            let waste = width * height;
            best = match best {
                Some((best_waste, _, _)) if best_waste <= waste => best,
                _ => Some((waste, cols, rows)),
            };
        }
    }
    let Some((_, cols, rows)) = best else {
        panic!(
            "no triple-grid block layout fits within {}x{}; reduce FONT_SIZE",
            MAX_SCENE_DIMENSION, MAX_SCENE_DIMENSION
        );
    };
    (cols, rows)
}

fn font_ref(font: &FontData) -> FontRef<'_> {
    let file_ref = FileRef::new(font.data.as_ref()).expect("Roboto font should decode");
    match file_ref {
        FileRef::Font(font_ref) => font_ref,
        FileRef::Collection(collection) => collection
            .get(font.index)
            .expect("Roboto font index should exist"),
    }
}

#[cfg(test)]
mod tests {
    use super::{TRIPLE_COUNT, triple_name};

    #[test]
    fn triple_order_is_row_major() {
        assert_eq!(triple_name(0), "aaa");
        assert_eq!(triple_name(13 * 26 * 26 + 7 * 26 + 5), "nhf");
        assert_eq!(triple_name(TRIPLE_COUNT - 1), "zzz");
    }
}
