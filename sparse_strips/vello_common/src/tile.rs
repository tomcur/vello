// Copyright 2025 the Vello Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Primitives for creating tiles.

use crate::flatten::{Line, Point};

/// The width of a tile.
pub const TILE_WIDTH: u32 = Tile::WIDTH as u32;
/// The height of a tile.
pub const TILE_HEIGHT: u32 = Tile::HEIGHT as u32;
const TILE_WIDTH_SCALE: f32 = TILE_WIDTH as f32;
const TILE_HEIGHT_SCALE: f32 = TILE_HEIGHT as f32;
const INV_TILE_WIDTH_SCALE: f32 = 1.0 / TILE_WIDTH_SCALE;
const INV_TILE_HEIGHT_SCALE: f32 = 1.0 / TILE_HEIGHT_SCALE;
// The value of 8192.0 is mainly chosen for compatibility with the old cpu-sparse
// implementation, where we scaled to u16.
const NUDGE_FACTOR: f32 = 1.0 / 8192.0;
const SCALED_X_NUDGE_FACTOR: f32 = 1.0 / (8192.0 * TILE_WIDTH_SCALE);

/// A tile represents an aligned area on the pixmap, used to subdivide the viewport into sub-areas
/// (currently 4x4) and analyze line intersections inside each such area.
///
/// Keep in mind that it is possible to have multiple tiles with the same index,
/// namely if we have multiple lines crossing the same 4x4 area!
#[derive(Debug, Clone, Copy)]
pub struct Tile {
    /// The index of the tile in the x direction.
    pub x: i32,
    /// The index of the tile in the y direction.
    pub y: u16,
    /// The index of the line this tile belongs to into the line buffer.
    pub line_idx: u32,
    pub winding: bool,
}

impl Tile {
    /// The width of a tile in pixels.
    pub const WIDTH: u16 = 4;

    /// The height of a tile in pixels.
    pub const HEIGHT: u16 = 4;

    /// Create a new tile.
    pub fn new(x: i32, y: u16, line_idx: u32, winding: bool) -> Self {
        Self {
            x,
            y,
            line_idx,
            winding,
        }
    }

    /// Check whether two tiles are at the same location.
    pub fn same_loc(&self, other: &Self) -> bool {
        self.x == other.x && self.same_row(other)
    }

    /// Check whether `self` is adjacent to the left of `other`.
    pub fn prev_loc(&self, other: &Self) -> bool {
        self.same_row(other) && self.x + 1 == other.x
    }

    /// Check whether two tiles are on the same row.
    pub fn same_row(&self, other: &Self) -> bool {
        self.y == other.y
    }
}

/// Handles the tiling of paths.
#[derive(Clone, Debug)]
pub struct Tiles {
    tile_buf: Vec<Tile>,
    tile_index_buf: Vec<TileIndex>,
    sorted: bool,
}

impl Default for Tiles {
    fn default() -> Self {
        Self::new()
    }
}

impl Tiles {
    /// Create a new tiles container.
    pub fn new() -> Self {
        Self {
            tile_buf: vec![],
            sorted: false,
            tile_index_buf: vec![],
        }
    }

    /// Get the number of tiles in the container.
    pub fn len(&self) -> u32 {
        self.tile_buf.len() as u32
    }

    /// Returns true if the container has no tiles.
    pub fn is_empty(&self) -> bool {
        self.tile_buf.is_empty()
    }

    /// Reset the tiles' container.
    pub fn reset(&mut self) {
        self.tile_buf.clear();
        self.tile_index_buf.clear();
        self.sorted = false;
    }

    /// Sort the tiles in the container.
    pub fn sort_tiles(&mut self) {
        self.sorted = true;
        self.tile_index_buf.sort_unstable_by(TileIndex::cmp);
    }

    /// Get the tile at a certain index.
    ///
    /// Panics if the container hasn't been sorted before.
    pub fn get(&self, index: u32) -> &Tile {
        assert!(
            self.sorted,
            "attempted to call `get` before sorting the tile container."
        );

        &self.tile_buf[self.tile_index_buf[index as usize].index()]
    }

    /// Iterate over the tiles in sorted order.
    ///
    /// Panics if the container hasn't been sorted before.
    pub fn iter(&self) -> impl Iterator<Item = &Tile> {
        assert!(
            self.sorted,
            "attempted to call `iter` before sorting the tile container."
        );

        self.tile_index_buf
            .iter()
            .map(|idx| &self.tile_buf[idx.index()])
    }

    /// Populate the tiles' container with a buffer of lines.
    ///
    /// Tiles outside the viewport (given by `width` and `height` in pixels) are culled.
    pub fn make_tiles(&mut self, lines: &[Line], width: u16, height: u16) {
        self.reset();

        if width == 0 || height == 0 {
            return;
        }

        debug_assert!(
            lines.len() < u32::MAX as usize + 1,
            "Max. number of lines per path exceeded. Max is {}, got {}.",
            u32::MAX,
            lines.len()
        );

        // Lines (partially) to the left of the viewport require some special handling, as these are
        // in front of the viewport from the perspective of the winding scan direction. These segments are put
        // into a tile with a vertical line at x=0 with the size of the y-delta to the left of the tile.
        //
        // TODO: if/when lines are removed from the tile packing, either the pre-viewport
        // fractional windings of each tile row should be calculated here and forwarded to strip
        // generation, or these special line segments should be pushed to the line soup.

        let tile_columns = width.div_ceil(Tile::WIDTH);
        let tile_rows = height.div_ceil(Tile::HEIGHT);

        for (line_idx, line) in lines.iter().take(u32::MAX as usize + 1).enumerate() {
            let line_idx = line_idx as u32;

            let p0_x = line.p0.x / Tile::WIDTH as f32;
            let p0_y = line.p0.y / Tile::HEIGHT as f32;
            let p1_x = line.p1.x / Tile::WIDTH as f32;
            let p1_y = line.p1.y / Tile::HEIGHT as f32;

            let (line_left_x, line_right_x) = if p0_x < p1_x {
                (p0_x, p1_x)
            } else {
                (p1_x, p0_x)
            };
            let (line_top_y, line_top_x, line_bottom_y, line_bottom_x) = if p0_y < p1_y {
                (p0_y, p0_x, p1_y, p1_x)
            } else {
                (p1_y, p1_x, p0_y, p0_x)
            };

            if line_left_x == line_right_x {
                let y_top_tiles = (line_top_y as u16).min(tile_rows);
                let y_bottom_tiles = (line_bottom_y as u16).min(tile_rows - 1);

                let x = line_left_x as u16;
                for y_idx in y_top_tiles..=y_bottom_tiles {
                    let tile = Tile::new(x as i32, y_idx, line_idx, y_idx != y_top_tiles);
                    self.tile_index_buf
                        .push(TileIndex::from_tile(self.tile_buf.len() as u32, &tile));
                    self.tile_buf.push(tile);
                }
            } else {
                let x_slope = (p1_x - p0_x) / (p1_y - p0_y);

                let y_top_tiles = (line_top_y as u16).min(tile_rows);
                let y_bottom_tiles = (line_bottom_y as u16).min(tile_rows - 1);

                // for y_idx in y_top_tiles..=y_bottom_tiles {
                //     let x = (line_left_x as u16).min(tile_columns);
                //     let xr = (line_right_x as u16).min(tile_columns - 1);
                //     for x_idx in x..=xr {
                //         let tile = Tile::new(x_idx as i32, y_idx, line_idx);
                //         self.tile_index_buf
                //             .push(TileIndex::from_tile(self.tile_buf.len() as u32, &tile));
                //         self.tile_buf.push(tile);
                //     }
                // }
                for y_idx in y_top_tiles..=y_bottom_tiles {
                    let y = y_idx as f32;

                    // The line's y-coordinates at the line's top-and bottom-most points within the
                    // tile row.
                    let line_row_top_y = line_top_y.max(y).min(y + 1.);
                    let line_row_bottom_y = line_bottom_y.max(y).min(y + 1.);

                    // The line's x-coordinates at the line's top- and bottom-most points within the
                    // tile row.
                    let line_row_top_x = p0_x + (line_row_top_y - p0_y) * x_slope;
                    let line_row_bottom_x = p0_x + (line_row_bottom_y - p0_y) * x_slope;

                    // The line's x-coordinates at the line's left- and right-most points within the
                    // tile row.
                    let line_row_left_x =
                        f32::min(line_row_top_x, line_row_bottom_x).max(line_left_x);
                    let line_row_right_x =
                        f32::max(line_row_top_x, line_row_bottom_x).min(line_right_x);

                    let winding_x = if line_top_x < line_bottom_x {
                        line_row_left_x as u16
                    } else {
                        line_row_right_x as u16
                    };

                    for x_idx in
                        line_row_left_x as u16..=(line_row_right_x as u16).min(tile_columns - 1)
                    {
                        let tile = Tile::new(
                            x_idx as i32,
                            y_idx,
                            line_idx,
                            y_idx != y_top_tiles && x_idx == winding_x,
                        );
                        self.tile_index_buf
                            .push(TileIndex::from_tile(self.tile_buf.len() as u32, &tile));
                        self.tile_buf.push(tile);
                    }
                }
            }
        }
    }
}

/// An index into a sorted tile buffer.
#[derive(Clone, Debug)]
struct TileIndex {
    x: u16,
    y: u16,
    index: u32,
}

impl TileIndex {
    pub(crate) fn from_tile(index: u32, tile: &Tile) -> Self {
        let x = (tile.x + 1).max(0) as u16;
        let y = tile.y;

        Self { x, y, index }
    }

    pub(crate) fn cmp(&self, b: &Self) -> std::cmp::Ordering {
        let xya = ((self.y as u32) << 16) + (self.x as u32);
        let xyb = ((b.y as u32) << 16) + (b.x as u32);
        xya.cmp(&xyb)
    }

    pub(crate) fn index(&self) -> usize {
        self.index as usize
    }
}

#[cfg(test)]
const _: () = if TILE_WIDTH_SCALE != TILE_HEIGHT_SCALE {
    panic!("Can only handle square tiles for now.");
};

/// Scale a tile coordinate to a viewport coordinate. Note this assumes tiles are square.
const fn scale_up(z: f32) -> f32 {
    z * TILE_WIDTH_SCALE
}

/// Scale a viewport coordinate to a tile coordinate.
const fn scale_down(z: Point) -> Point {
    Point::new(z.x * INV_TILE_WIDTH_SCALE, z.y * INV_TILE_HEIGHT_SCALE)
}

#[cfg(test)]
mod tests {
    use crate::flatten::{Line, Point};
    use crate::tile::Tiles;

    #[test]
    fn issue_46_infinite_loop() {
        let line = Line {
            p0: Point { x: 22.0, y: 552.0 },
            p1: Point { x: 224.0, y: 388.0 },
        };

        let mut tiles = Tiles::new();
        tiles.make_tiles(&[line]);
    }
}
