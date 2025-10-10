use std::borrow::Borrow;
use bytemuck::{Pod, Zeroable};
use szu::iter::MultiZipArrayExt;
use wgpu::util::DeviceExt;
use futures::executor::block_on;

use crate::video::{oc_color::RGB8, Image};

use super::{SIZE, WIDTH, HEIGHT, BITS};

#[repr(C, packed)]
#[derive(Clone, Copy, Pod, Zeroable, Debug)]
pub struct RGB8Packed {
	pub r: u8,
	pub g: u8,
	pub b: u8,
}

#[repr(C, packed)]
#[derive(Clone, Copy, Pod, Zeroable, Debug)]
pub struct Output {
	pub id: u32,
	pub bg: RGB8Packed,
	pub fg: RGB8Packed,
	pub score: i32,
}

pub struct Braille<T> {
	pub id: u8,
	pub bg: T,
	pub fg: T,
}

impl Braille<RGB8> {
	/// GPU-accelerated replacement for `from_pixels`
	async fn from_pixels_gpu(pixels: &[impl Borrow<[RGB8Packed; WIDTH]>; HEIGHT]) -> Self {
		// Flatten pixels into linear buffer
		let flat: Vec<RGB8> = pixels.iter()
			.flat_map(|row| row.borrow().iter().copied())
			.collect();

		// Initialize WGPU
		let instance = wgpu::Instance::default();
		let adapter = instance.request_adapter(&wgpu::RequestAdapterOptions::default())
			.await.unwrap();
		let (device, queue) = adapter.request_device(&Default::default()).await.unwrap();

		// Create buffers
		let pixel_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
			label: Some("Pixel Buffer"),
			contents: bytemuck::cast_slice(&flat),
			usage: wgpu::BufferUsages::STORAGE,
		});

		let output_buffer = device.create_buffer(&wgpu::BufferDescriptor {
			label: Some("Output Buffer"),
			size: (std::mem::size_of::<Output>() * (1 << (BITS - 1))) as u64,
			usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
			mapped_at_creation: false,
		});

		// Load WGSL shader
		let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
			label: Some("Compute Shader"),
			source: wgpu::ShaderSource::Wgsl(include_str!("compute.wgsl").into()),
		});

		// Create pipeline + bind group
		let compute_pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
			label: Some("Compute Pipeline"),
			layout: None,
			module: &shader,
			entry_point: Some("main"),
			cache: None,
			compilation_options: Default::default(),
		});

		let bind_group_layout = compute_pipeline.get_bind_group_layout(0);
		let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
			layout: &bind_group_layout,
			entries: &[
				wgpu::BindGroupEntry {
					binding: 0,
					resource: pixel_buffer.as_entire_binding(),
				},
				wgpu::BindGroupEntry {
					binding: 1,
					resource: output_buffer.as_entire_binding(),
				},
			],
			label: Some("Bind Group"),
		});

		// Encode compute pass
		let mut encoder = device.create_command_encoder(&Default::default());
		{
			let mut pass = encoder.begin_compute_pass(&Default::default());
			pass.set_pipeline(&compute_pipeline);
			pass.set_bind_group(0, &bind_group, &[]);
			pass.dispatch_workgroups((1 << (BITS - 1)) as u32, 1, 1);
		}
		queue.submit(Some(encoder.finish()));

		// Read back
		let buffer_slice = output_buffer.slice(..);
		let (sender, receiver) = futures_intrusive::channel::shared::oneshot_channel();
		buffer_slice.map_async(wgpu::MapMode::Read, move |v| sender.send(v).unwrap());
		device.poll(wgpu::PollType::Wait { submission_index: None, timeout: None });
		receiver.receive().await.unwrap().unwrap();

		let data = buffer_slice.get_mapped_range();
		let outputs: &[Output] = bytemuck::cast_slice(&data);
		let best = outputs.iter().max_by_key(|o| o.score).unwrap().clone();

		drop(data);
		output_buffer.unmap();

		Self {
			id: best.id as u8,
			bg: best.bg,
			fg: best.fg,
		}
	}
}

pub fn as_braille(input: &Image<RGB8>) -> Image<Braille<RGB8>> {
	// Build pixel clusters (same logic as before)
	let braille_pixel_clusters = input.buffer()
		.chunks_exact(input.size().x)
		.array_chunks::<{ HEIGHT }>()
		.map(|char_row| char_row
			.map(|row| row
				.array_chunks::<{ WIDTH }>())
			.multi_zip_array());

	let buffer: Vec<Braille<RGB8>> = braille_pixel_clusters
		.flat_map(|rows| rows.map(|cluster| block_on(from_pixels_gpu(&cluster))))
		.collect();

	Image::new(
		*input.size() / SIZE,
		buffer,
	)
}
