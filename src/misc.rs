use std::usize;
use crate::texture_allocator::*;
use crate::{DeviceIntSize, DeviceIntRect};

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
    fn add_texture(&mut self, _size: DeviceIntSize) -> TextureId {
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
    pub textures: Vec<TexturePage>,
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


