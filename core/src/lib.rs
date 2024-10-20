use ft_vox_prototype_0_map_core::Map;
use ft_vox_prototype_0_map_types::{Chunk, CHUNK_SIZE};
use ft_vox_prototype_0_util_lru_cache_rc::LRUCache;
use glam::{Mat3, Vec3};
use std::{collections::HashMap, rc::Rc};
use wgpu::util::DeviceExt;

mod vertex;
mod vox_graphics_wrapper;

use vertex::create_vertices_for_chunk;
use vox_graphics_wrapper::*;

pub const RENDER_DISTANCE: f32 = 22.0;

pub trait TerrainWorker {
    fn new(map: Map, render_distance: f32) -> Self;
    fn get_available(
        &mut self,
        chunk_coords: &[(i32, i32, i32)],
    ) -> Vec<((i32, i32, i32), Rc<Chunk>)>;
}

pub fn get_coords(distance: f32) -> Vec<(i32, i32, i32)> {
    let mut coords = Vec::new();
    let max_coord = distance.floor() as i32;
    let distance_squared = distance * distance;

    for x in -max_coord..=max_coord {
        for y in -max_coord..=max_coord {
            for z in -max_coord..=max_coord {
                let dist_sq = (x * x + y * y + z * z) as f32;
                if dist_sq <= distance_squared {
                    coords.push((x, y, z));
                }
            }
        }
    }

    coords
}

pub struct Vox<T: TerrainWorker> {
    vox_graphics_wrapper: VoxGraphicsWrapper,
    eye: glam::Vec3,
    horizontal_rotation: f32,
    vertical_rotation: f32,
    is_paused: bool,
    chunks: HashMap<[i32; 3], Rc<Chunk>>,
    buffers: LRUCache<[i32; 3], Rc<(wgpu::Buffer, wgpu::Buffer, u32)>>,
    terrain_worker: T,
}

/// [ Speed in Minecraft ]
/// Walking speed: 4.317 blocks/second
/// Sprinting speed (Survival): 5.612 blocks/second
/// Flying speed (Creative): 10.89 blocks/second

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum MoveSpeed {
    WALK,
    FLY,
}

impl MoveSpeed {
    pub const fn speed_per_sec(&self) -> f32 {
        match self {
            Self::WALK => 4.317,
            Self::FLY => 10.89,
        }
    }
}

impl<T: TerrainWorker> Vox<T> {
    pub fn init(
        config: &wgpu::SurfaceConfiguration,
        _adapter: &wgpu::Adapter,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
    ) -> Self {
        let vox_graphics_wrapper = VoxGraphicsWrapper::init(config, _adapter, device, queue);

        // Done
        Vox {
            vox_graphics_wrapper,
            eye: glam::Vec3::new(0.0, -5.0, 3.0),
            horizontal_rotation: 0.0,
            vertical_rotation: 0.0,
            chunks: HashMap::new(),
            buffers: LRUCache::new(get_coords(RENDER_DISTANCE).len()),
            is_paused: false,
            terrain_worker: T::new(Map::new(42), RENDER_DISTANCE),
        }
    }

    pub fn is_paused(&self) -> bool {
        self.is_paused
    }

    pub fn set_is_paused(&mut self, paused: bool) {
        self.is_paused = paused;
    }

    pub fn resize(
        &mut self,
        config: &wgpu::SurfaceConfiguration,
        device: &wgpu::Device,
        _queue: &wgpu::Queue,
    ) {
        self.vox_graphics_wrapper.resize(config, device, _queue);
    }

    pub fn tick(
        &mut self,
        delta_time: f32,
        move_direction: [f32; 3],
        move_speed: MoveSpeed,
        delta_horizontal_rotation: f32,
        delta_vertical_rotation: f32,
    ) {
        if self.is_paused {
            return;
        }

        // rotate
        {
            self.horizontal_rotation += delta_horizontal_rotation;
            self.horizontal_rotation %= 2.0 * std::f32::consts::PI;
            if self.horizontal_rotation < 0.0 {
                self.horizontal_rotation += 2.0 * std::f32::consts::PI;
            }

            self.vertical_rotation += delta_vertical_rotation;
            self.vertical_rotation = self.vertical_rotation.clamp(
                -0.4999 * std::f32::consts::PI,
                0.4999 * std::f32::consts::PI,
            );
        }

        // move
        {
            let move_direction = {
                let move_direction = Mat3::from_rotation_z(self.horizontal_rotation)
                    * Vec3::new(move_direction[0], move_direction[1], move_direction[2]);
                let move_speed = move_direction.length();
                if move_speed > 1.0 {
                    move_direction / move_speed
                } else {
                    move_direction
                }
            };
            self.eye += move_direction * move_speed.speed_per_sec() * delta_time;
        }
    }

    pub fn render(&mut self, view: &wgpu::TextureView, device: &wgpu::Device, queue: &wgpu::Queue) {
        let (eye_x, eye_y, eye_z) = {
            let eye = (self.eye / CHUNK_SIZE as f32).floor();
            (eye.x as i32, eye.y as i32, eye.z as i32)
        };

        for ((x, y, z), chunk) in self.terrain_worker.get_available(
            &get_coords(RENDER_DISTANCE + 2.0)
                .into_iter()
                .map(|(x, y, z)| (x + eye_x, y + eye_y, z + eye_z))
                .collect::<Vec<_>>(),
        ) {
            self.chunks.insert([x, y, z], chunk);
        }

        let buffer_data = get_coords(RENDER_DISTANCE)
            .into_iter()
            .map(|(x, y, z)| (x + eye_x, y + eye_y, z + eye_z))
            .collect::<Vec<_>>()
            .iter()
            .filter_map(|&(x, y, z)| {
                self.get_buffers(device, x, y, z)
                    .map(|buffers| (x, y, z, buffers))
            })
            .collect::<Vec<_>>();

        self.vox_graphics_wrapper.render(
            view,
            device,
            queue,
            self.eye,
            self.horizontal_rotation,
            self.vertical_rotation,
            buffer_data,
        );
    }

    fn get_buffers(
        &mut self,
        device: &wgpu::Device,
        x: i32,
        y: i32,
        z: i32,
    ) -> Option<Rc<(wgpu::Buffer, wgpu::Buffer, u32)>> {
        if let Some(result) = self.buffers.get(&[x, y, z]) {
            Some(result)
        } else if let Some(new_value) = self.create_buffers(device, x, y, z) {
            let result = Rc::new(new_value);
            self.buffers.put([x, y, z], result.clone());
            Some(result)
        } else {
            None
        }
    }

    fn create_buffers(
        &mut self,
        device: &wgpu::Device,
        x: i32,
        y: i32,
        z: i32,
    ) -> Option<(wgpu::Buffer, wgpu::Buffer, u32)> {
        let chunk = self.chunks.get(&[x, y, z]);
        let chunk_px = self.chunks.get(&[x + 1, y, z]);
        let chunk_nx = self.chunks.get(&[x - 1, y, z]);
        let chunk_py = self.chunks.get(&[x, y + 1, z]);
        let chunk_ny = self.chunks.get(&[x, y - 1, z]);
        let chunk_pz = self.chunks.get(&[x, y, z + 1]);
        let chunk_nz = self.chunks.get(&[x, y, z - 1]);

        if chunk.is_none()
            || chunk_px.is_none()
            || chunk_nx.is_none()
            || chunk_py.is_none()
            || chunk_ny.is_none()
            || chunk_pz.is_none()
            || chunk_nz.is_none()
        {
            None
        } else {
            let chunk = chunk.unwrap();
            let chunk_px = chunk_px.unwrap();
            let chunk_nx = chunk_nx.unwrap();
            let chunk_py = chunk_py.unwrap();
            let chunk_ny = chunk_ny.unwrap();
            let chunk_pz = chunk_pz.unwrap();
            let chunk_nz = chunk_nz.unwrap();

            let (vertex_data, index_data) = create_vertices_for_chunk(
                chunk, x, y, z, chunk_px, chunk_nx, chunk_py, chunk_ny, chunk_pz, chunk_nz,
            );

            let vertex_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Vertex Buffer"),
                contents: bytemuck::cast_slice(&vertex_data),
                usage: wgpu::BufferUsages::VERTEX,
            });

            let index_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Index Buffer"),
                contents: bytemuck::cast_slice(&index_data),
                usage: wgpu::BufferUsages::INDEX,
            });

            Some((vertex_buf, index_buf, index_data.len() as u32))
        }
    }
}
