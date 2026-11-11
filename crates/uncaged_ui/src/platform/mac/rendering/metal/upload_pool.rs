//! Reusable GPU upload memory for per-frame instance data.
//!
//! Every frame uploads rect, glyph and image instance data. Creating a fresh `MTLBuffer` for each
//! of those (as the renderer used to) leaves the driver holding the freed allocations as "owned
//! physical footprint (graphics)" for a while: about 120 MB at startup, counted against the app in
//! Activity Monitor. Instead, each frame bump-allocates out of a few shared buffers that are handed
//! back once the GPU has finished the command buffer that used them.

use std::mem;
use std::ptr::NonNull;

use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2_metal::{
    MTLBuffer, MTLCommandBuffer, MTLCommandBufferStatus, MTLDevice, MTLResourceOptions,
};

/// Size of a new arena. Enough for a typical full-window frame; arenas grow when a frame needs more.
const MIN_ARENA_BYTES: usize = 256 * 1024;
/// Buffer offsets bound for vertex data must be aligned; 256 bytes satisfies every Metal GPU family.
const OFFSET_ALIGNMENT: usize = 256;
/// Free arenas kept around for reuse. Three frames can be in flight, so a few more are never used.
const MAX_FREE_ARENAS: usize = 4;

type CommandBuffer = Retained<ProtocolObject<dyn MTLCommandBuffer>>;

struct Arena {
    buffer: Retained<ProtocolObject<dyn MTLBuffer>>,
    capacity: usize,
    used: usize,
}

impl Arena {
    fn new(device: &ProtocolObject<dyn MTLDevice>, capacity: usize) -> Self {
        let buffer = device
            .newBufferWithLength_options(capacity, MTLResourceOptions::StorageModeShared)
            .expect("device should create an upload buffer");
        Self {
            buffer,
            capacity,
            used: 0,
        }
    }
}

#[derive(Default)]
pub(super) struct UploadPool {
    /// Arenas the GPU is done with.
    free: Vec<Arena>,
    /// Arenas used by the frame being encoded; the last one is being filled.
    current: Vec<Arena>,
    /// Arenas used by committed frames, with the command buffer that reads them.
    in_flight: Vec<(CommandBuffer, Vec<Arena>)>,
}

impl UploadPool {
    /// Copies `data` into upload memory and returns the buffer and offset to bind it at.
    pub(super) fn upload<T>(
        &mut self,
        device: &ProtocolObject<dyn MTLDevice>,
        data: &[T],
    ) -> (Retained<ProtocolObject<dyn MTLBuffer>>, usize) {
        let len = mem::size_of_val(data);
        let fits =
            |arena: &Arena| arena.used.next_multiple_of(OFFSET_ALIGNMENT) + len <= arena.capacity;
        if !self.current.last().is_some_and(fits) {
            self.reclaim_completed();
            let arena = match self.free.iter().position(|arena| len <= arena.capacity) {
                Some(index) => self.free.swap_remove(index),
                None => {
                    let grown = self.current.last().map_or(0, |arena| arena.capacity * 2);
                    Arena::new(
                        device,
                        len.max(grown).max(MIN_ARENA_BYTES).next_power_of_two(),
                    )
                }
            };
            self.current.push(arena);
        }

        let arena = self.current.last_mut().expect("an arena was just ensured");
        let offset = arena.used.next_multiple_of(OFFSET_ALIGNMENT);
        // SAFETY: `fits` guarantees `offset + len <= capacity`, the buffer uses shared storage so
        // its contents are CPU-addressable, and the GPU is not reading this arena: it is either
        // fresh or was reclaimed after the command buffer that used it completed.
        unsafe {
            let dst = arena.buffer.contents().as_ptr().cast::<u8>().add(offset);
            std::ptr::copy_nonoverlapping(NonNull::from(data).cast::<u8>().as_ptr(), dst, len);
        }
        arena.used = offset + len;
        (arena.buffer.clone(), offset)
    }

    /// Hands the arenas used by the frame just encoded to `command_buffer`; they are reused once it
    /// completes.
    pub(super) fn end_frame(&mut self, command_buffer: CommandBuffer) {
        if !self.current.is_empty() {
            self.in_flight
                .push((command_buffer, mem::take(&mut self.current)));
        }
    }

    fn reclaim_completed(&mut self) {
        let mut index = 0;
        while index < self.in_flight.len() {
            let status = self.in_flight[index].0.status();
            if status == MTLCommandBufferStatus::Completed
                || status == MTLCommandBufferStatus::Error
            {
                let (_, arenas) = self.in_flight.swap_remove(index);
                for mut arena in arenas {
                    arena.used = 0;
                    self.free.push(arena);
                }
            } else {
                index += 1;
            }
        }
        // Steady rendering cycles through a handful of arenas; drop any beyond that.
        self.free.truncate(MAX_FREE_ARENAS);
    }
}
