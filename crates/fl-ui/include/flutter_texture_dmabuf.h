// fl-engine's dma-buf external textures: a GPU-surface type for Flutter's
// desktop texture registrar (flutter_texture_registrar.h), beside the
// header's Windows types and ivi-homescreen's GL texture.
//
// A producer registers a texture of kFlutterDesktopGpuSurfaceTexture whose
// FlutterDesktopGpuSurfaceTextureConfig has
//
//   type = kFlutterDesktopGpuSurfaceTypeDmaBuf
//
// and whose callback returns a FlutterDesktopGpuSurfaceDescriptor with
//
//   handle          a FlutterDesktopDmaBuf*
//   width, height   the buffer's size
//   visible_width,  what is drawn of it, from its top left corner
//   visible_height
//   format          not read; the dma-buf's fourcc says it
//
// The callback runs on the raster thread when it draws a frame that shows the
// texture, after the producer has marked one available. Everything the
// descriptor points to is lent until its release_callback runs, which is
// before the callback's frame is drawn: by then the engine has its own
// duplicates of the planes' descriptors and has waited on the acquire fence.
//
// The engine imports each buffer once and samples it where it lies, keeping
// the last four buffers a texture handed over: a producer cycling through a
// pool of buffers hands over the same descriptors (or duplicates of them)
// again, and each is recognized by its dma-buf, not its descriptor number.
// The engine does not say when it has finished reading a buffer: one written
// while it is drawn may show partly written. A pool of buffers written in
// turn keeps a write away from the buffer last handed over.
//
// The pixels are premultiplied, as a pixel buffer's are. Formats drawn:
// DRM_FORMAT_ARGB8888 and DRM_FORMAT_ABGR8888, linear or at any modifier the
// device samples.

#ifndef FL_ENGINE_FLUTTER_TEXTURE_DMABUF_H_
#define FL_ENGINE_FLUTTER_TEXTURE_DMABUF_H_

#include <stddef.h>
#include <stdint.h>

#if defined(__cplusplus)
extern "C" {
#endif

// The FlutterDesktopGpuSurfaceType of a dma-buf.
#define kFlutterDesktopGpuSurfaceTypeDmaBuf 4

#define FL_DMA_BUF_MAX_PLANES 4

typedef struct {
  // sizeof(FlutterDesktopDmaBuf).
  size_t struct_size;
  // DRM_FORMAT_ARGB8888 or DRM_FORMAT_ABGR8888.
  uint32_t fourcc;
  // The planes the modifier lays the buffer out in, 1 to 4.
  uint32_t plane_count;
  // DRM_FORMAT_MOD_LINEAR, or the layout the buffer was allocated with.
  uint64_t modifier;
  // Each plane's dma-buf descriptor, offset and pitch, in bytes. Lent.
  int32_t fds[FL_DMA_BUF_MAX_PLANES];
  uint32_t offsets[FL_DMA_BUF_MAX_PLANES];
  uint32_t pitches[FL_DMA_BUF_MAX_PLANES];
  // A sync_file that signals when the frame is written, or -1 where it
  // already is. Lent. The engine waits up to a second for it, and gives the
  // frame up past that.
  int32_t acquire_fence;
} FlutterDesktopDmaBuf;

#if defined(__cplusplus)
}  // extern "C"
#endif

#endif  // FL_ENGINE_FLUTTER_TEXTURE_DMABUF_H_
