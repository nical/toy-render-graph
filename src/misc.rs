use std::usize;
use std::i32;
use std::collections::HashMap;
use crate::texture_allocator::*;

#[repr(transparent)]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct NodeId(u32);

impl NodeId {
    pub fn to_usize(self) -> usize { self.0 as usize }
}

pub(crate) fn node_id(idx: usize) -> NodeId {
    debug_assert!(idx < std::u32::MAX as usize);
    NodeId(idx as u32)
}

#[repr(transparent)]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct TextureId(pub(crate) u32);

impl TextureId {
    pub fn to_usize(self) -> usize { self.0 as usize }
}

pub(crate) fn texture_id(idx: usize) -> TextureId {
    debug_assert!(idx < std::u32::MAX as usize);
    TextureId(idx as u32)
}

pub trait AtlasAllocator {
    fn add_texture(&mut self, size: DeviceIntSize) -> TextureId;
    fn allocate(&mut self, tex: TextureId, size: DeviceIntSize) -> DeviceIntRect;
    fn deallocate(&mut self, tex: TextureId, rect: &DeviceIntRect);
    fn flush_deallocations(&mut self, _texture_id: TextureId) {}
}

pub struct DummyAtlasAllocator {
    tex: u32,
}

impl DummyAtlasAllocator {
    pub fn new() -> Self {
        DummyAtlasAllocator { tex: 0 }
    }
}

impl AtlasAllocator for DummyAtlasAllocator {
    fn add_texture(&mut self, size: DeviceIntSize) -> TextureId {
        let id = self.tex;
        self.tex += 1;
        TextureId(id)
    }

    fn allocate(&mut self, texture_id: TextureId, _size: DeviceIntSize) -> DeviceIntRect {
        assert!(texture_id.0 < self.tex);
        DeviceIntRect::zero()
    }

    fn deallocate(&mut self, texture_id: TextureId, _rect: &DeviceIntRect) {
        assert!(texture_id.0 < self.tex);
    }
}

pub struct GuillotineAllocator {
    textures: Vec<TexturePage>,
}

impl GuillotineAllocator {
    pub fn new() -> Self {
        GuillotineAllocator {
            textures: Vec::new(),
        }
    }
}

impl AtlasAllocator for GuillotineAllocator {

    fn add_texture(&mut self, size: DeviceIntSize) -> TextureId {
        self.textures.push(TexturePage::new(size));
        texture_id(self.textures.len() - 1)
    }

    fn allocate(&mut self, texture_id: TextureId, size: DeviceIntSize) -> DeviceIntRect {
        DeviceIntRect {
            origin: self.textures[texture_id.to_usize()].allocate(&size).unwrap(),
            size,
        }
    }

    fn deallocate(&mut self, texture_id: TextureId, rect: &DeviceIntRect) {
        self.textures[texture_id.to_usize()].free(&rect);
    }

    fn flush_deallocations(&mut self, texture_id: TextureId) {
        self.textures[texture_id.to_usize()].coalesce();
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct DeviceIntSize {
    pub width: i32,
    pub height: i32,
}

impl DeviceIntSize {
    pub fn new(width: i32, height: i32) -> Self {
        DeviceIntSize { width, height }
    }

    pub fn area(&self) -> i32 { self.width * self.height }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct DeviceIntPoint {
    pub x: i32,
    pub y: i32,
}

impl DeviceIntPoint {
    pub fn new(x: i32, y: i32) -> Self {
        DeviceIntPoint { x, y }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct DeviceIntBox2D {
    pub min: DeviceIntPoint,
    pub max: DeviceIntPoint,
}

impl DeviceIntBox2D {
    pub fn zero() -> Self {
        DeviceIntBox2D {
            min: DeviceIntPoint { x: 0, y: 0 },
            max: DeviceIntPoint { x: 0, y: 0 },
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct DeviceIntRect {
    pub origin: DeviceIntPoint,
    pub size: DeviceIntSize,
}

impl DeviceIntRect {
    pub fn new(origin: DeviceIntPoint, size: DeviceIntSize) -> Self {
        Self { origin, size }
    }

    pub fn zero() -> Self {
        Self {
            origin: DeviceIntPoint { x: 0, y: 0 },
            size: size2(0, 0),
        }
    }

    pub fn min_x(&self) -> i32 { self.origin.x }
    pub fn min_y(&self) -> i32 { self.origin.y }
    pub fn max_x(&self) -> i32 { self.origin.x + self.size.width }
    pub fn max_y(&self) -> i32 { self.origin.y + self.size.height }

    pub fn union(&self, other: &Self) -> Self {
        if self.size == size2(0, 0) {
            return *other;
        }
        if other.size == size2(0, 0) {
            return *self;
        }

        use std::i32;
        let upper_left = DeviceIntPoint::new(
            i32::min(self.min_x(), other.min_x()),
            i32::min(self.min_y(), other.min_y()),
        );

        let lower_right_x = i32::max(self.max_x(), other.max_x());
        let lower_right_y = i32::max(self.max_y(), other.max_y());

        DeviceIntRect::new(
            upper_left,
            DeviceIntSize::new(lower_right_x - upper_left.x, lower_right_y - upper_left.y),
        )
    }
}

pub fn size2(width: i32, height: i32) -> DeviceIntSize {
    DeviceIntSize { width, height }
}

impl std::convert::From<DeviceIntSize> for DeviceIntRect {
    fn from(size: DeviceIntSize) -> Self {
        DeviceIntRect {
            origin: DeviceIntPoint { x: 0, y: 0 },
            size,
        }
    }
}
