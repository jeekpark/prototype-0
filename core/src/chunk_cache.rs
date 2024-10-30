use std::{
    collections::{HashSet, VecDeque},
    sync::{Arc, Mutex},
};

use ft_vox_prototype_0_map_types::{Chunk, CHUNK_SIZE};

use crate::{get_coords, TerrainWorker};
use crate::{vertex::Vertex, TerrainWorkerJob};

pub struct TerrainManager<W: TerrainWorker, D: Clone + Send + 'static> {
    map_cache: Arc<Mutex<MapCache>>,
    mesh_cache: Arc<Mutex<MeshCache<D>>>,
    eye: (f32, f32, f32),
    terrain_worker: W,
}

struct MapCache {
    pub chunk_loading: HashSet<(i32, i32, i32)>,
    pub chunks: Vec<Option<Arc<Chunk>>>,

    pub cache_distance: usize,
    pub coords: Vec<(i32, i32, i32)>,
    pub x: i32,
    pub y: i32,
    pub z: i32,
    pub eye_x_upper: bool,
    pub eye_y_upper: bool,
    pub eye_z_upper: bool,
}

impl MapCache {
    pub fn new(cache_distance: usize, eye: (f32, f32, f32)) -> Self {
        let size = cache_distance * 2 + 2;
        let (x, y, z) = eye;

        MapCache {
            chunk_loading: HashSet::new(),
            chunks: vec![None; size * size * size],
            cache_distance,
            coords: calculate_coords(cache_distance as f32),
            x: (x / CHUNK_SIZE as f32).floor() as i32,
            y: (y / CHUNK_SIZE as f32).floor() as i32,
            z: (z / CHUNK_SIZE as f32).floor() as i32,
            eye_x_upper: x % CHUNK_SIZE as f32 > CHUNK_SIZE as f32 / 2.0,
            eye_y_upper: y % CHUNK_SIZE as f32 > CHUNK_SIZE as f32 / 2.0,
            eye_z_upper: z % CHUNK_SIZE as f32 > CHUNK_SIZE as f32 / 2.0,
        }
    }
    pub fn get(&self, x: i32, y: i32, z: i32) -> Option<Arc<Chunk>> {
        let size = self.cache_distance * 2 + 2;

        let min_x = self.x - self.cache_distance as i32 - if self.eye_x_upper { 0 } else { 1 };
        let max_x = self.x + self.cache_distance as i32 + if self.eye_x_upper { 1 } else { 0 };
        if min_x > x || x > max_x {
            return None;
        }
        let x = x.rem_euclid(size as i32) as usize;

        let min_y = self.y - self.cache_distance as i32 - if self.eye_y_upper { 0 } else { 1 };
        let max_y = self.y + self.cache_distance as i32 + if self.eye_y_upper { 1 } else { 0 };
        if min_y > y || y > max_y {
            return None;
        }
        let y = y.rem_euclid(size as i32) as usize;

        let min_z = self.z - self.cache_distance as i32 - if self.eye_z_upper { 0 } else { 1 };
        let max_z = self.z + self.cache_distance as i32 + if self.eye_z_upper { 1 } else { 0 };
        if min_z > z || z > max_z {
            return None;
        }
        let z = z.rem_euclid(size as i32) as usize;

        self.chunks[z * size * size + y * size + x].clone()
    }

    pub fn set(&mut self, x: i32, y: i32, z: i32, chunk: Option<Arc<Chunk>>) {
        let size = self.cache_distance * 2 + 2;

        let min_x = self.x - self.cache_distance as i32 - if self.eye_x_upper { 0 } else { 1 };
        let max_x = self.x + self.cache_distance as i32 + if self.eye_x_upper { 1 } else { 0 };
        if min_x > x || x > max_x {
            return;
        }
        let x = x.rem_euclid(size as i32) as usize;

        let min_y = self.y - self.cache_distance as i32 - if self.eye_y_upper { 0 } else { 1 };
        let max_y = self.y + self.cache_distance as i32 + if self.eye_y_upper { 1 } else { 0 };
        if min_y > y || y > max_y {
            return;
        }
        let y = y.rem_euclid(size as i32) as usize;

        let min_z = self.z - self.cache_distance as i32 - if self.eye_z_upper { 0 } else { 1 };
        let max_z = self.z + self.cache_distance as i32 + if self.eye_z_upper { 1 } else { 0 };
        if min_z > z || z > max_z {
            return;
        }
        let z = z.rem_euclid(size as i32) as usize;

        self.chunks[z * size * size + y * size + x] = chunk;
    }

    fn reset(&mut self) {
        let size = self.cache_distance * 2 + 2;
        self.chunks = vec![None; size * size * size];
        self.chunk_loading.clear();
    }

    fn get_available(&self) -> Vec<((i32, i32, i32), Arc<Chunk>)> {
        self.coords
            .iter()
            .map(|&(x, y, z)| (x + self.x, y + self.y, z + self.z))
            .filter_map(|(x, y, z)| self.get(x, y, z).map(|chunk| ((x, y, z), chunk)))
            .collect()
    }
}

#[derive(Clone)]
enum MeshCacheItem<T: Clone + Send + 'static> {
    Raw {
        vertices: Vec<Vertex>,
        indices: Vec<u16>,
    },
    Processed(T),
}

impl<T: Clone + Send + 'static> MeshCacheItem<T> {
    fn is_raw(&self) -> bool {
        matches!(self, MeshCacheItem::Raw { .. })
    }

    fn is_processed(&self) -> bool {
        matches!(self, MeshCacheItem::Processed(..))
    }

    fn as_raw(self) -> Option<(Vec<Vertex>, Vec<u16>)> {
        if let MeshCacheItem::Raw { vertices, indices } = self {
            Some((vertices, indices))
        } else {
            None
        }
    }
}

struct MeshCache<T: Clone + Send + 'static> {
    pub mesh_load_request: VecDeque<((i32, i32, i32), Vec<Arc<Chunk>>)>,
    pub meshes: Vec<Option<MeshCacheItem<T>>>,

    pub cache_distance: usize,
    pub coords: Vec<(i32, i32, i32)>,
    pub x: i32,
    pub y: i32,
    pub z: i32,
    pub eye_x_upper: bool,
    pub eye_y_upper: bool,
    pub eye_z_upper: bool,
}

impl<T: Clone + Send + 'static> MeshCache<T> {
    pub fn new(cache_distance: usize, eye: (f32, f32, f32)) -> Self {
        let size = cache_distance * 2 + 2;
        let (x, y, z) = eye;

        MeshCache {
            mesh_load_request: VecDeque::new(),
            meshes: vec![None; size * size * size],
            cache_distance,
            coords: calculate_coords(cache_distance as f32),
            x: (x / CHUNK_SIZE as f32).floor() as i32,
            y: (y / CHUNK_SIZE as f32).floor() as i32,
            z: (z / CHUNK_SIZE as f32).floor() as i32,
            eye_x_upper: x % CHUNK_SIZE as f32 > CHUNK_SIZE as f32 / 2.0,
            eye_y_upper: y % CHUNK_SIZE as f32 > CHUNK_SIZE as f32 / 2.0,
            eye_z_upper: z % CHUNK_SIZE as f32 > CHUNK_SIZE as f32 / 2.0,
        }
    }

    pub fn get_processed(
        &mut self,
        x: i32,
        y: i32,
        z: i32,
        process: &mut dyn FnMut(Vec<Vertex>, Vec<u16>) -> T,
    ) -> Option<T> {
        let size = self.cache_distance * 2 + 2;

        let min_x = self.x - self.cache_distance as i32 - if self.eye_x_upper { 0 } else { 1 };
        let max_x = self.x + self.cache_distance as i32 + if self.eye_x_upper { 1 } else { 0 };
        if min_x > x || x > max_x {
            return None;
        }
        let x = x.rem_euclid(size as i32) as usize;

        let min_y = self.y - self.cache_distance as i32 - if self.eye_y_upper { 0 } else { 1 };
        let max_y = self.y + self.cache_distance as i32 + if self.eye_y_upper { 1 } else { 0 };
        if min_y > y || y > max_y {
            return None;
        }
        let y = y.rem_euclid(size as i32) as usize;

        let min_z = self.z - self.cache_distance as i32 - if self.eye_z_upper { 0 } else { 1 };
        let max_z = self.z + self.cache_distance as i32 + if self.eye_z_upper { 1 } else { 0 };
        if min_z > z || z > max_z {
            return None;
        }
        let z = z.rem_euclid(size as i32) as usize;

        if let Some(item) = &self.meshes[z * size * size + y * size + x] {
            if let MeshCacheItem::Processed(result) = item {
                Some(result.clone())
            } else {
                let (vertices, indices) = self.meshes[z * size * size + y * size + x]
                    .take()
                    .unwrap()
                    .as_raw()
                    .unwrap();
                let result = process(vertices, indices);
                self.meshes[z * size * size + y * size + x] =
                    Some(MeshCacheItem::Processed(result.clone()));
                Some(result)
            }
        } else {
            None
        }
    }

    pub fn set(&mut self, x: i32, y: i32, z: i32, mesh: Option<(Vec<Vertex>, Vec<u16>)>) {
        let size = self.cache_distance * 2 + 2;

        let min_x = self.x - self.cache_distance as i32 - if self.eye_x_upper { 0 } else { 1 };
        let max_x = self.x + self.cache_distance as i32 + if self.eye_x_upper { 1 } else { 0 };
        if min_x > x || x > max_x {
            return;
        }
        let x = x.rem_euclid(size as i32) as usize;

        let min_y = self.y - self.cache_distance as i32 - if self.eye_y_upper { 0 } else { 1 };
        let max_y = self.y + self.cache_distance as i32 + if self.eye_y_upper { 1 } else { 0 };
        if min_y > y || y > max_y {
            return;
        }
        let y = y.rem_euclid(size as i32) as usize;

        let min_z = self.z - self.cache_distance as i32 - if self.eye_z_upper { 0 } else { 1 };
        let max_z = self.z + self.cache_distance as i32 + if self.eye_z_upper { 1 } else { 0 };
        if min_z > z || z > max_z {
            return;
        }
        let z = z.rem_euclid(size as i32) as usize;

        self.meshes[z * size * size + y * size + x] =
            mesh.map(|(vertices, indices)| MeshCacheItem::Raw { vertices, indices });
    }

    fn reset(&mut self) {
        let size = self.cache_distance * 2 + 2;
        self.meshes = vec![None; size * size * size];
        self.mesh_load_request.clear();
    }

    fn get_available(
        &mut self,
        process: &mut dyn FnMut(Vec<Vertex>, Vec<u16>) -> T,
    ) -> Vec<((i32, i32, i32), T)> {
        let coords = self
            .coords
            .iter()
            .map(|&(x, y, z)| (x + self.x, y + self.y, z + self.z))
            .collect::<Vec<_>>();
        coords
            .into_iter()
            .filter_map(|(x, y, z)| {
                self.get_processed(x, y, z, process)
                    .map(|mesh| ((x, y, z), mesh))
            })
            .collect()
    }
}

impl<W: TerrainWorker, D: Clone + Send + 'static> TerrainManager<W, D> {
    pub fn new(cache_distance: usize, eye: (f32, f32, f32)) -> Self {
        let mut result = Self {
            map_cache: Arc::new(Mutex::new(MapCache::new(cache_distance, eye))),
            mesh_cache: Arc::new(Mutex::new(MeshCache::new(cache_distance, eye))),
            eye,
            terrain_worker: W::new(
                Arc::new(Mutex::new(|| None)),
                Arc::new(Mutex::new(|_pos, _chunk| ())),
                Arc::new(Mutex::new(|_pos, _mesh| ())),
            ),
        };
        result.init();

        result
    }

    fn init(&mut self) {
        self.terrain_worker = W::new(
            Arc::new(Mutex::new({
                let map_cache = self.map_cache.clone();
                let mesh_cache = self.mesh_cache.clone();
                move || {
                    let mut map_cache = map_cache.lock().unwrap();
                    if let Some((position, vec)) =
                        mesh_cache.lock().unwrap().mesh_load_request.pop_front()
                    {
                        return Some(TerrainWorkerJob::Mesh {
                            position,
                            zero: vec[0].clone(),
                            positive_x: vec[1].clone(),
                            negative_x: vec[2].clone(),
                            positive_y: vec[3].clone(),
                            negative_y: vec[4].clone(),
                            positive_z: vec[5].clone(),
                            negative_z: vec[6].clone(),
                        });
                    }
                    let result = map_cache
                        .coords
                        .iter()
                        .map(|&(x, y, z)| (x + map_cache.x, y + map_cache.y, z + map_cache.z))
                        .find(|&(x, y, z)| {
                            map_cache.get(x, y, z).is_none()
                                && !map_cache.chunk_loading.contains(&(x, y, z))
                        });
                    if let Some(pos) = result {
                        map_cache.chunk_loading.insert(pos);
                        return Some(TerrainWorkerJob::Map(pos));
                    }
                    None
                }
            })),
            Arc::new(Mutex::new({
                let map_cache = self.map_cache.clone();
                let mesh_cache = self.mesh_cache.clone();
                move |(x, y, z), chunk| {
                    let mut map_cache = map_cache.lock().unwrap();

                    map_cache.chunk_loading.remove(&(x, y, z));
                    map_cache.set(x, y, z, Some(chunk));
                    let directions = [
                        (1, 0, 0),  // x+1
                        (-1, 0, 0), // x-1
                        (0, 1, 0),  // y+1
                        (0, -1, 0), // y-1
                        (0, 0, 1),  // z+1
                        (0, 0, -1), // z-1
                    ];
                    for (dx, dy, dz) in directions.iter() {
                        if let Some(chunk) = map_cache.get(x + dx, y + dy, z + dz) {
                            let mut chunks7: Vec<Arc<Chunk>> = Vec::new();

                            chunks7.push(chunk.clone());

                            for (sub_dx, sub_dy, sub_dz) in directions.iter() {
                                if let Some(sub_chunk) =
                                    map_cache.get(x + dx + sub_dx, y + dy + sub_dy, z + dz + sub_dz)
                                {
                                    chunks7.push(sub_chunk.clone());
                                }
                            }

                            if chunks7.len() == 7 {
                                let mut mesh_cache = mesh_cache.lock().unwrap();
                                mesh_cache
                                    .mesh_load_request
                                    .push_back(((x + dx, y + dy, z + dz), chunks7));
                            }
                        }
                    }
                }
            })),
            Arc::new(Mutex::new({
                let mesh_cache = self.mesh_cache.clone();
                move |(x, y, z), mesh| {
                    let mut mesh_cache = mesh_cache.lock().unwrap();
                    mesh_cache.set(x, y, z, Some(mesh));
                }
            })),
        );
    }

    pub fn set_cache_distance(&mut self, new_cache_distance: usize) {
        {
            let mut map_cache = self.map_cache.lock().unwrap();
            if map_cache.cache_distance != new_cache_distance {
                map_cache.cache_distance = new_cache_distance;
                map_cache.coords = calculate_coords(map_cache.cache_distance as f32);
                map_cache.reset();
            }
        }

        {
            let mut mesh_cache = self.mesh_cache.lock().unwrap();
            if mesh_cache.cache_distance != new_cache_distance {
                mesh_cache.cache_distance = new_cache_distance;
                mesh_cache.coords = calculate_coords(mesh_cache.cache_distance as f32);
                mesh_cache.reset();
            }
        }

        // TODO: add resize without reset
    }

    pub fn set_eye(&mut self, eye: (f32, f32, f32)) {
        let mut map_cache = self.map_cache.lock().unwrap();
        let mut mesh_cache = self.mesh_cache.lock().unwrap();
        fn upper(value: f32, old: bool) -> bool {
            let value = (value.fract() + 1.0).fract();
            if old {
                value > 0.25
            } else {
                value > 0.75
            }
        }
        let size = map_cache.cache_distance * 2 + 2;
        let old_eye_chunk_x = map_cache.x;
        let old_eye_chunk_y = map_cache.y;
        let old_eye_chunk_z = map_cache.z;
        let old_eye_x_upper = map_cache.eye_x_upper;
        let old_eye_y_upper = map_cache.eye_y_upper;
        let old_eye_z_upper = map_cache.eye_z_upper;
        let old_min_x =
            old_eye_chunk_x - map_cache.cache_distance as i32 - if old_eye_x_upper { 0 } else { 1 };
        let old_min_y =
            old_eye_chunk_y - map_cache.cache_distance as i32 - if old_eye_y_upper { 0 } else { 1 };
        let old_min_z =
            old_eye_chunk_z - map_cache.cache_distance as i32 - if old_eye_z_upper { 0 } else { 1 };
        let (new_eye_x, new_eye_y, new_eye_z) = eye;
        let new_eye_chunk_x = (new_eye_x / CHUNK_SIZE as f32).floor() as i32;
        let new_eye_chunk_y = (new_eye_y / CHUNK_SIZE as f32).floor() as i32;
        let new_eye_chunk_z = (new_eye_z / CHUNK_SIZE as f32).floor() as i32;
        let new_eye_x_upper = upper(new_eye_x / CHUNK_SIZE as f32, old_eye_x_upper);
        let new_eye_y_upper = upper(new_eye_y / CHUNK_SIZE as f32, old_eye_y_upper);
        let new_eye_z_upper = upper(new_eye_z / CHUNK_SIZE as f32, old_eye_z_upper);
        let new_min_x =
            new_eye_chunk_x - map_cache.cache_distance as i32 - if new_eye_x_upper { 0 } else { 1 };
        let new_min_y =
            new_eye_chunk_y - map_cache.cache_distance as i32 - if new_eye_y_upper { 0 } else { 1 };
        let new_min_z =
            new_eye_chunk_z - map_cache.cache_distance as i32 - if new_eye_z_upper { 0 } else { 1 };
        let new_max_x = new_min_x + size as i32 - 1;
        let new_max_y = new_min_y + size as i32 - 1;
        let new_max_z = new_min_z + size as i32 - 1;

        self.eye = eye;
        map_cache.eye_x_upper = new_eye_x_upper;
        map_cache.eye_y_upper = new_eye_y_upper;
        map_cache.eye_z_upper = new_eye_z_upper;
        map_cache.x = new_eye_chunk_x;
        map_cache.y = new_eye_chunk_y;
        map_cache.z = new_eye_chunk_z;

        mesh_cache.eye_x_upper = new_eye_x_upper;
        mesh_cache.eye_y_upper = new_eye_y_upper;
        mesh_cache.eye_z_upper = new_eye_z_upper;
        mesh_cache.x = new_eye_chunk_x;
        mesh_cache.y = new_eye_chunk_y;
        mesh_cache.z = new_eye_chunk_z;

        match new_min_x - old_min_x {
            0 => {}
            1 => {
                for z in 0..size {
                    for y in 0..size {
                        let x = new_max_x.rem_euclid(size as i32) as usize;
                        map_cache.chunks[z * size * size + y * size + x] = None;

                        mesh_cache.meshes[z * size * size + y * size + x] = None;
                    }
                }
            }
            -1 => {
                for z in 0..size {
                    for y in 0..size {
                        let x = new_min_x.rem_euclid(size as i32) as usize;
                        map_cache.chunks[z * size * size + y * size + x] = None;

                        mesh_cache.meshes[z * size * size + y * size + x] = None;
                    }
                }
            }
            _ => {
                map_cache.reset();
                return;
            }
        }

        match new_min_y - old_min_y {
            0 => {}
            1 => {
                for z in 0..size {
                    for x in 0..size {
                        let y = new_max_y.rem_euclid(size as i32) as usize;
                        map_cache.chunks[z * size * size + y * size + x] = None;

                        mesh_cache.meshes[z * size * size + y * size + x] = None;
                    }
                }
            }
            -1 => {
                for z in 0..size {
                    for x in 0..size {
                        let y = new_min_y.rem_euclid(size as i32) as usize;
                        map_cache.chunks[z * size * size + y * size + x] = None;

                        mesh_cache.meshes[z * size * size + y * size + x] = None;
                    }
                }
            }
            _ => {
                map_cache.reset();
                mesh_cache.reset();
                return;
            }
        }

        match new_min_z - old_min_z {
            0 => {}
            1 => {
                for x in 0..size {
                    for y in 0..size {
                        let z = new_max_z.rem_euclid(size as i32) as usize;
                        map_cache.chunks[z * size * size + y * size + x] = None;

                        mesh_cache.meshes[z * size * size + y * size + x] = None;
                    }
                }
            }
            -1 => {
                for x in 0..size {
                    for y in 0..size {
                        let z = new_min_z.rem_euclid(size as i32) as usize;
                        map_cache.chunks[z * size * size + y * size + x] = None;

                        mesh_cache.meshes[z * size * size + y * size + x] = None;
                    }
                }
            }
            _ => {
                map_cache.reset();
                mesh_cache.reset();
                // return;
            }
        }
    }

    pub fn get_available(
        &mut self,
        process: &mut dyn FnMut(Vec<Vertex>, Vec<u16>) -> D,
    ) -> Vec<((i32, i32, i32), D)> {
        self.mesh_cache.lock().unwrap().get_available(process)
    }
}

fn calculate_coords(distance: f32) -> Vec<(i32, i32, i32)> {
    let mut result = get_coords(distance);

    fn dst((x, y, z): (i32, i32, i32)) -> i32 {
        x * x + y * y + z * z
    }
    result.sort_unstable_by(|&a, &b| dst(a).cmp(&dst(b)));

    result
}
