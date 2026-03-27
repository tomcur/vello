// Copyright 2025 the Vello Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Benchmark lowercase glyph-triplet rendering through `fill_texture_rects` versus `glyph_run`.

#[path = "support/glyph_pair_support.rs"]
mod glyph_pair_support;

use std::io::Cursor;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use glyph_pair_support::{
    DEFAULT_ITERATIONS, DEFAULT_WARMUP, GlyphPairData, PAIR_COUNT, SceneDimensions, TRIPLE_COUNT,
    atlas_output_path,
};
use vello_common::color::palette::css::{BLACK, WHITE};
use vello_common::kurbo::Rect;
use vello_common::peniko::ImageQuality;
use vello_hybrid::{
    Pixmap, RenderSize, RenderTargetConfig, Resources, SampleRect, Scene, TextureBindings,
    TextureId,
};

const GLYPH_PAIR_ATLAS_PNG: &[u8] = include_bytes!("assets/glyphs_roboto_lowercase.png");

fn main() {
    pollster::block_on(run());
}

async fn run() {
    let options = Options::parse();
    let pair_data = GlyphPairData::new();
    let (scene_size, glyphs, texture_rects, cell_count) = if options.small {
        (
            pair_data.pair_scene_dimensions(),
            pair_data.pair_glyphs(),
            pair_data.pair_texture_rects(),
            PAIR_COUNT,
        )
    } else {
        (
            pair_data.scene_dimensions(),
            pair_data.scene_glyphs(),
            pair_data.texture_rects(),
            TRIPLE_COUNT,
        )
    };

    let atlas_texture = load_premultiplied_rgba_png(GLYPH_PAIR_ATLAS_PNG);
    assert_eq!(
        (atlas_texture.width, atlas_texture.height),
        (
            u32::from(pair_data.layout.atlas_width),
            u32::from(pair_data.layout.atlas_height)
        ),
        "embedded atlas dimensions do not match the shared glyph-atlas layout; regenerate {}",
        atlas_output_path().display(),
    );

    let gpu = GpuContext::new(scene_size, &atlas_texture).await;

    let mode_label = if options.small { "pair" } else { "triple" };
    println!(
        "Benchmarking {} {} cells via {} texture rects ({} outline glyphs, viewport {}x{})",
        cell_count,
        mode_label,
        texture_rects.len(),
        glyphs.len(),
        scene_size.width,
        scene_size.height,
    );
    let scene_cell_width = if options.small {
        pair_data.layout.pair_cell_width
    } else {
        pair_data.layout.triple_cell_width
    };
    println!(
        "Glyph atlas cell {}x{}, scene cell {}x{}, atlas {}x{}",
        pair_data.layout.glyph_cell_width,
        pair_data.layout.cell_height,
        scene_cell_width,
        pair_data.layout.cell_height,
        pair_data.layout.atlas_width,
        pair_data.layout.atlas_height,
    );

    // --- texture_rects ---
    let mut texture_frame_scene = Scene::new(scene_size.width, scene_size.height);
    let texture_frame_total =
        measure_frame_total(&gpu, &options, &mut texture_frame_scene, |scene| {
            build_texture_scene(scene, scene_size, &texture_rects)
        });
    let mut texture_render_scene = Scene::new(scene_size.width, scene_size.height);
    build_texture_scene(&mut texture_render_scene, scene_size, &texture_rects);
    let texture_render_only = measure_render_only(&gpu, &options, &texture_render_scene);

    // --- glyph_run (no atlas cache) ---
    let mut glyph_frame_scene = Scene::new(scene_size.width, scene_size.height);
    let glyph_frame_total = measure_frame_total(&gpu, &options, &mut glyph_frame_scene, |scene| {
        let mut resources = gpu.resources.borrow_mut();
        build_glyph_scene(
            scene,
            scene_size,
            &pair_data.font,
            &glyphs,
            &mut resources,
            false,
        );
    });
    let mut glyph_render_scene = Scene::new(scene_size.width, scene_size.height);
    {
        let mut resources = gpu.resources.borrow_mut();
        build_glyph_scene(
            &mut glyph_render_scene,
            scene_size,
            &pair_data.font,
            &glyphs,
            &mut resources,
            false,
        );
    }
    let glyph_render_only = measure_render_only(&gpu, &options, &glyph_render_scene);

    // --- glyph_run with atlas cache ---
    let mut atlas_frame_scene = Scene::new(scene_size.width, scene_size.height);
    let atlas_frame_total = measure_frame_total(&gpu, &options, &mut atlas_frame_scene, |scene| {
        let mut resources = gpu.resources.borrow_mut();
        build_glyph_scene(
            scene,
            scene_size,
            &pair_data.font,
            &glyphs,
            &mut resources,
            true,
        );
    });

    // --- Results ---
    println!();
    print_result("texture_rects   frame_total", texture_frame_total);
    print_result("glyph_run       frame_total", glyph_frame_total);
    print_result("glyph_run (atl) frame_total", atlas_frame_total);
    println!(
        "frame_total glyph_run       vs texture_rects: {:.2}x",
        glyph_frame_total.avg().as_secs_f64() / texture_frame_total.avg().as_secs_f64()
    );
    println!(
        "frame_total glyph_run (atl) vs texture_rects: {:.2}x",
        atlas_frame_total.avg().as_secs_f64() / texture_frame_total.avg().as_secs_f64()
    );
    println!();
    print_result("texture_rects   render_only", texture_render_only);
    print_result("glyph_run       render_only", glyph_render_only);
    println!(
        "render_only glyph_run vs texture_rects: {:.2}x",
        glyph_render_only.avg().as_secs_f64() / texture_render_only.avg().as_secs_f64()
    );

    if let Some(output_path) = &options.output {
        render_scene_to_png(&gpu, &texture_render_scene, output_path);
        println!("\nWrote pair grid to {}", output_path.display());
    }
}

fn build_texture_scene(scene: &mut Scene, size: SceneDimensions, texture_rects: &[SampleRect]) {
    scene.reset();
    scene.set_paint(WHITE);
    scene.fill_rect(&Rect::new(
        0.0,
        0.0,
        f64::from(size.width),
        f64::from(size.height),
    ));
    scene.draw_texture_rects(
        TextureId(0),
        ImageQuality::Low,
        texture_rects.iter().copied(),
    );
}

fn build_glyph_scene(
    scene: &mut Scene,
    size: SceneDimensions,
    font: &vello_common::peniko::FontData,
    glyphs: &[glifo::Glyph],
    resources: &mut Resources,
    use_atlas_cache: bool,
) {
    scene.reset();
    scene.set_paint(WHITE);
    scene.fill_rect(&Rect::new(
        0.0,
        0.0,
        f64::from(size.width),
        f64::from(size.height),
    ));
    scene.set_paint(BLACK);
    scene
        .glyph_run(resources, font)
        .font_size(glyph_pair_support::FONT_SIZE)
        .hint(true)
        .atlas_cache(use_atlas_cache)
        .fill_glyphs(glyphs.iter().copied());
}

fn measure_frame_total<FBuild>(
    gpu: &GpuContext,
    options: &Options,
    scene: &mut Scene,
    mut build_scene: FBuild,
) -> BenchmarkResult
where
    FBuild: FnMut(&mut Scene),
{
    for _ in 0..options.warmup {
        build_scene(scene);
        render_scene(gpu, scene);
    }

    let mut best = Duration::MAX;
    let total_start = Instant::now();
    for _ in 0..options.iterations {
        let start = Instant::now();
        build_scene(scene);
        render_scene(gpu, scene);
        best = best.min(start.elapsed());
    }

    BenchmarkResult {
        total: total_start.elapsed(),
        best,
        iterations: options.iterations,
    }
}

fn measure_render_only(gpu: &GpuContext, options: &Options, scene: &Scene) -> BenchmarkResult {
    for _ in 0..options.warmup {
        render_scene(gpu, scene);
    }

    let mut best = Duration::MAX;
    let total_start = Instant::now();
    for _ in 0..options.iterations {
        let start = Instant::now();
        render_scene(gpu, scene);
        best = best.min(start.elapsed());
    }

    BenchmarkResult {
        total: total_start.elapsed(),
        best,
        iterations: options.iterations,
    }
}

fn render_scene(gpu: &GpuContext, scene: &Scene) {
    let mut encoder = gpu
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Glyph Pair Benchmark Encoder"),
        });
    render_scene_with_encoder(gpu, scene, &mut encoder);
    gpu.queue.submit([encoder.finish()]);
    gpu.device
        .poll(wgpu::PollType::wait_indefinitely())
        .expect("GPU work should complete");
}

fn render_scene_to_png(gpu: &GpuContext, scene: &Scene, output_path: &PathBuf) {
    let mut encoder = gpu
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Glyph Pair Benchmark Output Encoder"),
        });
    render_scene_with_encoder(gpu, scene, &mut encoder);

    let bytes_per_row = (gpu.render_size.width * 4).next_multiple_of(256);
    let readback = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("Glyph Pair Benchmark Readback"),
        size: u64::from(bytes_per_row) * u64::from(gpu.render_size.height),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: &gpu._target_texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &readback,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(bytes_per_row),
                rows_per_image: None,
            },
        },
        wgpu::Extent3d {
            width: gpu.render_size.width,
            height: gpu.render_size.height,
            depth_or_array_layers: 1,
        },
    );
    gpu.queue.submit([encoder.finish()]);
    readback
        .slice(..)
        .map_async(wgpu::MapMode::Read, move |result| {
            if result.is_err() {
                panic!("failed to map benchmark output buffer");
            }
        });
    gpu.device
        .poll(wgpu::PollType::wait_indefinitely())
        .expect("GPU work should complete");

    let width = gpu.render_size.width as usize;
    let mut pixels = Vec::with_capacity(width * gpu.render_size.height as usize);
    for row in readback
        .slice(..)
        .get_mapped_range()
        .chunks_exact(bytes_per_row as usize)
    {
        pixels.extend_from_slice(bytemuck::cast_slice(&row[..width * 4]));
    }
    readback.unmap();

    let pixmap = Pixmap::from_parts(
        pixels,
        gpu.render_size.width as u16,
        gpu.render_size.height as u16,
    );
    let encoded = pixmap
        .into_png()
        .expect("failed to encode benchmark output PNG");
    std::fs::write(output_path, encoded).expect("failed to write benchmark output PNG");
}

fn render_scene_with_encoder(gpu: &GpuContext, scene: &Scene, encoder: &mut wgpu::CommandEncoder) {
    let mut texture_bindings = TextureBindings::new();
    texture_bindings.insert(TextureId(0), &gpu.source_view);
    gpu.renderer
        .borrow_mut()
        .render_with_texture_bindings(
            scene,
            &mut gpu.resources.borrow_mut(),
            &gpu.device,
            &gpu.queue,
            encoder,
            &gpu.render_size,
            &gpu.target_view,
            &texture_bindings,
        )
        .expect("benchmark render should succeed");
}

fn print_result(label: &str, result: BenchmarkResult) {
    println!(
        "{label}: avg {:>8.3} ms, best {:>8.3} ms",
        result.avg().as_secs_f64() * 1_000.0,
        result.best.as_secs_f64() * 1_000.0,
    );
}

#[derive(Clone)]
struct Options {
    iterations: u32,
    warmup: u32,
    small: bool,
    output: Option<PathBuf>,
}

impl Options {
    fn parse() -> Self {
        let mut options = Self {
            iterations: DEFAULT_ITERATIONS,
            warmup: DEFAULT_WARMUP,
            small: false,
            output: None,
        };

        let mut args = std::env::args().skip(1);
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--iterations" => {
                    options.iterations = args
                        .next()
                        .expect("--iterations requires a value")
                        .parse()
                        .expect("--iterations must be a positive integer");
                }
                "--warmup" => {
                    options.warmup = args
                        .next()
                        .expect("--warmup requires a value")
                        .parse()
                        .expect("--warmup must be a non-negative integer");
                }
                "--small" => {
                    options.small = true;
                }
                "--output" => {
                    options.output = Some(args.next().expect("--output requires a path").into());
                }
                _ => panic!("unknown argument: {arg}"),
            }
        }

        assert!(
            options.iterations > 0,
            "--iterations must be greater than zero"
        );
        options
    }
}

#[derive(Clone, Copy)]
struct BenchmarkResult {
    total: Duration,
    best: Duration,
    iterations: u32,
}

impl BenchmarkResult {
    fn avg(self) -> Duration {
        self.total / self.iterations
    }
}

struct GpuContext {
    device: wgpu::Device,
    queue: wgpu::Queue,
    renderer: std::cell::RefCell<vello_hybrid::Renderer>,
    resources: std::cell::RefCell<Resources>,
    render_size: RenderSize,
    _target_texture: wgpu::Texture,
    target_view: wgpu::TextureView,
    _source_texture: wgpu::Texture,
    source_view: wgpu::TextureView,
}

impl GpuContext {
    async fn new(scene_size: SceneDimensions, atlas: &PremultipliedRgbaTexture) -> Self {
        let instance = wgpu::Instance::default();
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions::default())
            .await
            .expect("failed to find a GPU adapter for benchmarking");
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("Glyph Pair Benchmark Device"),
                ..Default::default()
            })
            .await
            .expect("failed to create benchmark device");

        let render_size = RenderSize {
            width: u32::from(scene_size.width),
            height: u32::from(scene_size.height),
        };
        let target_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Glyph Pair Benchmark Target"),
            size: wgpu::Extent3d {
                width: render_size.width,
                height: render_size.height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let target_view = target_texture.create_view(&wgpu::TextureViewDescriptor::default());

        let source_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Glyph Pair Atlas"),
            size: wgpu::Extent3d {
                width: atlas.width,
                height: atlas.height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &source_texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &atlas.rgba,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(atlas.width * 4),
                rows_per_image: None,
            },
            wgpu::Extent3d {
                width: atlas.width,
                height: atlas.height,
                depth_or_array_layers: 1,
            },
        );
        let source_view = source_texture.create_view(&wgpu::TextureViewDescriptor::default());

        let renderer = vello_hybrid::Renderer::new(
            &device,
            &RenderTargetConfig {
                format: wgpu::TextureFormat::Rgba8Unorm,
                width: render_size.width,
                height: render_size.height,
            },
        );

        Self {
            device,
            queue,
            renderer: std::cell::RefCell::new(renderer),
            resources: std::cell::RefCell::new(Resources::new()),
            render_size,
            _target_texture: target_texture,
            target_view,
            _source_texture: source_texture,
            source_view,
        }
    }
}

struct PremultipliedRgbaTexture {
    rgba: Vec<u8>,
    width: u32,
    height: u32,
}

fn load_premultiplied_rgba_png(bytes: &[u8]) -> PremultipliedRgbaTexture {
    let mut decoder = png::Decoder::new(Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::ALPHA | png::Transformations::STRIP_16);

    let mut reader = decoder
        .read_info()
        .expect("failed to decode glyph pair atlas PNG");
    assert_eq!(
        reader.output_color_type(),
        (png::ColorType::Rgba, png::BitDepth::Eight),
        "expected the glyph pair atlas to be RGBA8"
    );

    let (width, height) = {
        let info = reader.info();
        (info.width, info.height)
    };
    let mut rgba = vec![0; reader.output_buffer_size().unwrap_or_default()];
    reader
        .next_frame(&mut rgba)
        .expect("failed to read glyph pair atlas PNG");

    for pixel in rgba.chunks_exact_mut(4) {
        let alpha = u16::from(pixel[3]);
        pixel[0] = ((u16::from(pixel[0]) * alpha + 128) / 255) as u8;
        pixel[1] = ((u16::from(pixel[1]) * alpha + 128) / 255) as u8;
        pixel[2] = ((u16::from(pixel[2]) * alpha + 128) / 255) as u8;
    }

    PremultipliedRgbaTexture {
        rgba,
        width,
        height,
    }
}
