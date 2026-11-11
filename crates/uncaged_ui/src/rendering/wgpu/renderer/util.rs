use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use wgpu::{
    Buffer, BufferAddress, BufferDescriptor, BufferUsages, Device, Queue, COPY_BUFFER_ALIGNMENT,
};

use super::Error;

/// Calls the provided function, capturing and returning any validation errors
/// detected by wgpu.
#[must_use]
pub fn with_error_scope<T>(
    device: &wgpu::Device,
    callback: impl FnOnce() -> T,
) -> (T, Option<Error>) {
    let error_scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
    let ret = callback();
    // On native platforms, the future returned by `pop_error_scope` resolves
    // immediately.  On wasm, it may take longer due to asynchronous browser
    // APIs, but it's necessary to wait here to know if it is safe to continue.
    let error_future = error_scope.pop();
    cfg_if::cfg_if! {
        if #[cfg(target_family = "wasm")] {
            let error = crate::r#async::block_on(error_future);
        } else {
            use futures::FutureExt;
            let error = error_future.now_or_never().expect("always resolves immediately");
        }
    }
    (ret, error.map(Into::into))
}

/// The smallest instance buffer allocated; grown in powers of two from here.
const MIN_INSTANCE_BUFFER_SIZE: BufferAddress = 64 * 1024;

/// A vertex buffer for per-frame instance data that is kept across frames and refilled with
/// [`wgpu::Queue::write_buffer`].
///
/// Creating a fresh, mapped buffer for every frame made each frame allocate and free GPU memory,
/// which drivers pool and keep charging to the app. wgpu orders a `write_buffer` after earlier
/// submissions that read the buffer, so a single buffer per pipeline is enough.
#[derive(Default)]
pub struct InstanceBuffer {
    buffer: Option<Buffer>,
}

impl InstanceBuffer {
    /// Uploads `contents`, growing the buffer if needed, and returns the buffer to bind.
    pub fn upload(
        &mut self,
        device: &Device,
        queue: &Queue,
        device_lost: &Arc<AtomicBool>,
        label: &'static str,
        contents: &[u8],
    ) -> Result<Buffer, Error> {
        // Copies must be a multiple of COPY_BUFFER_ALIGNMENT.
        let align_mask = COPY_BUFFER_ALIGNMENT - 1;
        let padded_size = (contents.len() as BufferAddress + align_mask) & !align_mask;

        if self
            .buffer
            .as_ref()
            .is_none_or(|buffer| buffer.size() < padded_size)
        {
            self.buffer = None;
            let size = padded_size
                .max(MIN_INSTANCE_BUFFER_SIZE)
                .next_power_of_two();
            self.buffer = Some(create_buffer(
                device,
                &BufferDescriptor {
                    label: Some(label),
                    size,
                    usage: BufferUsages::VERTEX | BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                },
            )?);
        }

        if device_lost.load(Ordering::SeqCst) {
            return Err(Error::DeviceLost);
        }

        let buffer = self.buffer.clone().expect("buffer was just ensured");
        if contents.len() as BufferAddress == padded_size {
            if !contents.is_empty() {
                queue.write_buffer(&buffer, 0, contents);
            }
        } else {
            let mut padded = contents.to_vec();
            padded.resize(padded_size as usize, 0);
            queue.write_buffer(&buffer, 0, &padded);
        }
        Ok(buffer)
    }
}

/// Creates a buffer using the given device and descriptor, synchronously
/// returning an error if the buffer could not be created successfully.
fn create_buffer(device: &Device, desc: &BufferDescriptor<'_>) -> Result<Buffer, Error> {
    let (buffer, error) = with_error_scope(device, || device.create_buffer(desc));

    match error {
        Some(error) => {
            log::warn!("Failed to create wgpu::Buffer: {error:#}");
            Err(error)
        }
        None => Ok(buffer),
    }
}
