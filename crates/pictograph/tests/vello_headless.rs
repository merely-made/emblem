// Copyright 2026 Mark Alan Boykin
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// SPDX-License-Identifier: MPL-2.0

#![cfg(all(feature = "vello", not(target_arch = "wasm32")))]

use emblem::{GradientKind, Host, Matrix, Paint, Palette, Rgba, Sink as _, Spread, Stop};
use netrender_device::WgpuDevice;
use netrender_vello::{
    AaConfig, AaSupport, RenderParams, Renderer, RendererOptions, Scene, kurbo::Affine,
    peniko::Color,
};
use pictograph::vello::{VelloSink, decode};

const DIM: u32 = 64;
const ACTION_INFO: [u8; 36] = [
    0x8a, 0x49, 0x56, 0x47, 0x03, 0x0b, 0x11, 0x51, 0x51, 0xb1, 0xb1, 0x35, 0x81, 0x59, 0x33, 0x59,
    0x81, 0x81, 0xa9, 0x35, 0x85, 0x95, 0x34, 0x7d, 0x95, 0x7d, 0x7d, 0x35, 0x85, 0x75, 0x34, 0x7d,
    0x75, 0x7d, 0x6d, 0x88,
];

fn renderer(device: &netrender_vello::wgpu::Device) -> Renderer {
    Renderer::new(
        device,
        RendererOptions {
            use_cpu: false,
            antialiasing_support: AaSupport::area_only(),
            num_init_threads: None,
            pipeline_cache: None,
        },
    )
    .expect("vello renderer")
}

fn render(device: &WgpuDevice, renderer: &mut Renderer, scene: &Scene) -> Vec<u8> {
    let target = device
        .core
        .device
        .create_texture(&netrender_vello::wgpu::TextureDescriptor {
            label: Some("pictograph D2 headless target"),
            size: netrender_vello::wgpu::Extent3d {
                width: DIM,
                height: DIM,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: netrender_vello::wgpu::TextureDimension::D2,
            format: netrender_vello::wgpu::TextureFormat::Rgba8Unorm,
            usage: netrender_vello::wgpu::TextureUsages::STORAGE_BINDING
                | netrender_vello::wgpu::TextureUsages::TEXTURE_BINDING
                | netrender_vello::wgpu::TextureUsages::COPY_SRC,
            view_formats: &[netrender_vello::wgpu::TextureFormat::Rgba8UnormSrgb],
        });
    let view = target.create_view(&netrender_vello::wgpu::TextureViewDescriptor {
        label: Some("pictograph D2 storage view"),
        format: Some(netrender_vello::wgpu::TextureFormat::Rgba8Unorm),
        ..Default::default()
    });

    renderer
        .render_to_texture(
            &device.core.device,
            &device.core.queue,
            scene,
            &view,
            &RenderParams {
                base_color: Color::from_rgba8(0, 0, 0, 0),
                width: DIM,
                height: DIM,
                antialiasing_method: AaConfig::Area,
            },
        )
        .expect("headless vello render");

    device.read_rgba8_texture(&target, DIM, DIM)
}

fn pixel(bytes: &[u8], x: u32, y: u32) -> [u8; 4] {
    let at = ((y * DIM + x) * 4) as usize;
    [bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]
}

fn count_color(bytes: &[u8], expected: [u8; 3]) -> usize {
    bytes
        .chunks_exact(4)
        .filter(|pixel| {
            pixel[3] >= 245
                && pixel[..3]
                    .iter()
                    .zip(expected)
                    .all(|(&actual, expected)| actual.abs_diff(expected) <= 10)
        })
        .count()
}

fn rectangle(sink: &mut VelloSink, radius: f32) {
    sink.move_to(-radius, -radius);
    sink.line_to(radius, -radius);
    sink.line_to(radius, radius);
    sink.line_to(-radius, radius);
    sink.close();
}

fn gradient_stops() -> Vec<Stop> {
    vec![
        Stop {
            offset: 0.0,
            color: Rgba::new(0xFF, 0, 0, 0xFF),
        },
        Stop {
            offset: 1.0,
            color: Rgba::new(0, 0, 0xFF, 0xFF),
        },
    ]
}

#[test]
fn action_info_and_derived_faces_reach_pixels_through_netrenders_vello() {
    let device = WgpuDevice::boot().expect("headless wgpu device");
    let mut renderer = renderer(&device.core.device);
    let placement = Some(Affine::translate((32.0, 32.0)));

    let action = decode(&ACTION_INFO, &Palette::default(), Host::default()).unwrap();
    let mut action_scene = Scene::new();
    action.append_to(&mut action_scene, placement);
    let action_pixels = render(&device, &mut renderer, &action_scene);
    assert!(
        action_pixels
            .chunks_exact(4)
            .filter(|pixel| pixel[3] > 200)
            .count()
            > 100,
        "the specification action/info icon must produce visible pixels"
    );

    let file = pictograph::derive(b"D2 headless palette receipt").unwrap();
    let red = Palette::new([Rgba::new(0xFF, 0, 0, 0xFF); 64]).unwrap();
    let blue = Palette::new([Rgba::new(0, 0, 0xFF, 0xFF); 64]).unwrap();
    let host = Host {
        height: DIM as f32,
        ..Host::default()
    };
    let red_graphic = decode(&file, &red, host).unwrap();
    let blue_graphic = decode(&file, &blue, host).unwrap();

    let mut red_scene = Scene::new();
    red_graphic.append_to(&mut red_scene, placement);
    let red_pixels = render(&device, &mut renderer, &red_scene);
    let mut blue_scene = Scene::new();
    blue_graphic.append_to(&mut blue_scene, placement);
    let blue_pixels = render(&device, &mut renderer, &blue_scene);

    assert_ne!(
        red_pixels, blue_pixels,
        "one byte file must re-theme at decode time"
    );
    assert!(count_color(&red_pixels, [255, 0, 0]) > 100);
    assert!(count_color(&blue_pixels, [0, 0, 255]) > 100);

    // Two same-winding nested contours fill the centre under non-zero winding
    // and leave a hole under even-odd. This makes the format rule visible at a
    // pixel rather than proving only that an enum value was passed.
    let mut sink = VelloSink::new();
    for radius in [20.0, 10.0] {
        rectangle(&mut sink, radius);
    }
    sink.fill(&Paint::Flat(Rgba::new(0xFF, 0, 0, 0xFF)));
    let winding_fragment = sink.into_scene().unwrap();
    let mut winding_scene = Scene::new();
    winding_scene.append(&winding_fragment, placement);
    let winding_pixels = render(&device, &mut renderer, &winding_scene);
    let centre = pixel(&winding_pixels, 32, 32);
    assert!(
        centre[0] > 245 && centre[3] > 245,
        "centre pixel was {centre:?}"
    );
}

#[test]
fn gradient_matrices_reach_pixels_in_iconvg_direction() {
    let device = WgpuDevice::boot().expect("headless wgpu device");
    let mut renderer = renderer(&device.core.device);
    let placement = Some(Affine::translate((32.0, 32.0)));

    let mut linear_sink = VelloSink::new();
    rectangle(&mut linear_sink, 20.0);
    linear_sink.fill(&Paint::Gradient {
        kind: GradientKind::Linear,
        spread: Spread::Pad,
        stops: gradient_stops(),
        // Graphic x = -20 maps to gradient x = 0; x = 20 maps to 1.
        matrix: Matrix([0.025, 0.0, 0.5, 0.0, 0.0, 0.0]),
    });
    let mut linear_scene = Scene::new();
    linear_scene.append(&linear_sink.into_scene().unwrap(), placement);
    let linear_pixels = render(&device, &mut renderer, &linear_scene);
    let left = pixel(&linear_pixels, 16, 32);
    let right = pixel(&linear_pixels, 48, 32);
    assert!(left[0] > left[2], "linear left pixel was {left:?}");
    assert!(right[2] > right[0], "linear right pixel was {right:?}");

    let mut radial_sink = VelloSink::new();
    rectangle(&mut radial_sink, 20.0);
    radial_sink.fill(&Paint::Gradient {
        kind: GradientKind::Radial,
        spread: Spread::Pad,
        stops: gradient_stops(),
        // Graphic radius 20 maps to gradient radius 1.
        matrix: Matrix([0.05, 0.0, 0.0, 0.0, 0.05, 0.0]),
    });
    let mut radial_scene = Scene::new();
    radial_scene.append(&radial_sink.into_scene().unwrap(), placement);
    let radial_pixels = render(&device, &mut renderer, &radial_scene);
    let centre = pixel(&radial_pixels, 32, 32);
    let edge = pixel(&radial_pixels, 48, 32);
    assert!(centre[0] > centre[2], "radial centre pixel was {centre:?}");
    assert!(edge[2] > edge[0], "radial edge pixel was {edge:?}");
}
