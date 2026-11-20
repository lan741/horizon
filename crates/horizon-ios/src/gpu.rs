//! The Metal surface and the one pipeline the platform layer draws with.
//!
//! The engine's renderer (`horizon-client-render`) opens its own device; this is the smaller path
//! the platform layer needs on its own: prove the Metal surface, the ProMotion frame clock and the
//! uniforms reaching the GPU on a real iPad before the world renderer is wired in behind it.
use std::sync::Arc;
use winit::window::Window;

/// What the shader reads for one frame.
#[derive(Clone, Copy, Debug, Default)]
pub struct FrameInput {
    /// Seconds since the app started, for the idle animation.
    pub time: f32,
    /// The stick's value, `[-1, 1]`.
    pub stick: [f32; 2],
    /// The look delta consumed this frame, for the reticle's feedback.
    pub look: [f32; 2],
    /// Up to two fingers: `x`, `y`, strength.
    pub points: [[f32; 3]; 2],
}

/// `resolution`, `time`, `count`, `stick`, `look`, two `vec4` touches: 64 bytes, std140-aligned.
const UNIFORM_BYTES: usize = 64;

pub struct Gpu {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    pipeline: wgpu::RenderPipeline,
    uniforms: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
}

impl Gpu {
    /// Open the Metal device on `window`'s layer and configure the swapchain.
    pub fn new(window: Arc<Window>) -> Result<Self, String> {
        let size = window.inner_size();
        let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
        descriptor.backends = wgpu::Backends::METAL;
        let instance = wgpu::Instance::new(descriptor);
        let surface = instance
            .create_surface(window.clone())
            .map_err(|error| format!("no surface for the window: {error}"))?;
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
            ..Default::default()
        }))
        .map_err(|error| format!("no Metal adapter: {error}"))?;
        let info = adapter.get_info();
        log::info!(
            "Horizon iOS: {} ({:?}, {:?})",
            info.name,
            info.device_type,
            info.backend
        );
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("horizon-ios"),
            ..Default::default()
        }))
        .map_err(|error| format!("no Metal device: {error}"))?;

        let caps = surface.get_capabilities(&adapter);
        let format = caps
            .formats
            .iter()
            .copied()
            .find(wgpu::TextureFormat::is_srgb)
            .unwrap_or_else(|| caps.formats[0]);
        log::info!(
            "Horizon iOS: swapchain {:?}, present modes {:?}, {}x{} @ {} dpi, refresh {} mHz",
            format,
            caps.present_modes,
            size.width,
            size.height,
            window.scale_factor(),
            window
                .current_monitor()
                .and_then(|monitor| monitor.refresh_rate_millihertz())
                .unwrap_or(0)
        );
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: size.width.max(1),
            height: size.height.max(1),
            // Vsynced: on a ProMotion display this presents at the panel's rate (120 Hz), and iOS
            // drops the rate when nothing changes.
            present_mode: wgpu::PresentMode::AutoVsync,
            desired_maximum_frame_latency: 2,
            alpha_mode: wgpu::CompositeAlphaMode::Opaque,
            view_formats: vec![],
            color_space: wgpu::SurfaceColorSpace::default(),
        };
        surface.configure(&device, &config);

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("platform-layer"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("platform-frame"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("platform-uniforms"),
            size: UNIFORM_BYTES as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("platform-frame"),
            layout: &layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniforms.as_entire_binding(),
            }],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("platform"),
            bind_group_layouts: &[Some(&layout)],
            ..Default::default()
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("platform"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            multiview_mask: None,
            cache: None,
        });

        Ok(Self {
            surface,
            device,
            queue,
            config,
            pipeline,
            uniforms,
            bind_group,
        })
    }

    pub fn size(&self) -> (u32, u32) {
        (self.config.width, self.config.height)
    }

    /// Follow the window: rotation, a resize from the system, the Dynamic Island inset.
    pub fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        if (width, height) == (self.config.width, self.config.height) {
            return;
        }
        self.config.width = width;
        self.config.height = height;
        self.surface.configure(&self.device, &self.config);
        log::info!("Horizon iOS: surface now {width}x{height}");
    }

    pub fn render(&mut self, input: &FrameInput) -> Result<(), String> {
        self.queue
            .write_buffer(&self.uniforms, 0, &uniform_bytes(input, self.size()));
        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(frame) => frame,
            // A frame that no longer matches the surface is still usable once more: present it,
            // then reconfigure so the next one is in step with the panel.
            wgpu::CurrentSurfaceTexture::Suboptimal(frame) => {
                self.surface.configure(&self.device, &self.config);
                frame
            }
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                self.surface.configure(&self.device, &self.config);
                return Ok(());
            }
            // The screen is off, or the compositor is not handing out frames: skip, do not fail.
            wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => {
                return Ok(());
            }
            wgpu::CurrentSurfaceTexture::Validation => {
                return Err("the surface rejected the frame (validation)".to_string());
            }
        };
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("platform-frame"),
            });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("platform"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.043,
                            g: 0.078,
                            b: 0.063,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &self.bind_group, &[]);
            pass.draw(0..3, 0..1);
        }
        self.queue.submit(Some(encoder.finish()));
        // wgpu 30 presents through the queue, which also bookkeeps the frame for the backend.
        self.queue.present(frame);
        Ok(())
    }
}

fn uniform_bytes(input: &FrameInput, size: (u32, u32)) -> [u8; UNIFORM_BYTES] {
    let mut bytes = [0u8; UNIFORM_BYTES];
    let mut put = |offset: usize, value: f32| {
        bytes[offset..offset + 4].copy_from_slice(&value.to_ne_bytes());
    };
    put(0, size.0 as f32);
    put(4, size.1 as f32);
    put(8, input.time);
    let active = input.points.iter().filter(|point| point[2] > 0.0).count() as f32;
    put(12, active);
    put(16, input.stick[0]);
    put(20, input.stick[1]);
    put(24, input.look[0]);
    put(28, input.look[1]);
    for (index, point) in input.points.iter().enumerate() {
        let base = 32 + index * 16;
        put(base, point[0]);
        put(base + 4, point[1]);
        put(base + 8, point[2]);
    }
    bytes
}

const SHADER: &str = r#"
struct Uniforms {
    resolution: vec2<f32>,
    time: f32,
    active: f32,
    stick: vec2<f32>,
    look: vec2<f32>,
    points: array<vec4<f32>, 2>,
};
@group(0) @binding(0) var<uniform> frame: Uniforms;

@vertex
fn vs(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    var corners = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>(3.0, -1.0),
        vec2<f32>(-1.0, 3.0),
    );
    return vec4<f32>(corners[index], 0.0, 1.0);
}

@fragment
fn fs(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let uv = position.xy / max(frame.resolution, vec2<f32>(1.0, 1.0));
    // Horizon's own greens, so the port is recognisable at a glance.
    var colour = mix(vec3<f32>(0.043, 0.078, 0.063), vec3<f32>(0.129, 0.212, 0.180), uv.y);
    colour = colour + 0.015 * sin(frame.time * 0.9 + uv.x * 5.0) * vec3<f32>(0.2, 0.5, 0.35);

    // The thumb stick: a ring at the left, its knob where the thumb pushed it.
    let anchor = vec2<f32>(0.20, 0.74);
    let ring = abs(distance(uv, anchor) - 0.10);
    colour = mix(colour, vec3<f32>(0.42, 0.90, 0.60), smoothstep(0.008, 0.0, ring) * 0.55);
    let knob = anchor + frame.stick * 0.075;
    colour = mix(colour, vec3<f32>(0.62, 1.0, 0.76), smoothstep(0.042, 0.028, distance(uv, knob)) * 0.75);

    // Every finger lights its own spot, briefly brighter while it moves.
    for (var index: u32 = 0u; index < 2u; index = index + 1u) {
        let point = frame.points[index];
        if (point.z <= 0.0) {
            continue;
        }
        let glow = smoothstep(0.13, 0.0, distance(uv, point.xy));
        colour = colour + glow * point.z * vec3<f32>(0.30, 0.78, 0.50);
    }

    // The look delta shows as a reticle that leans the way the camera turned.
    let reticle = vec2<f32>(0.5) + frame.look * 0.02;
    let cross = abs(distance(uv, reticle) - 0.012);
    colour = mix(colour, vec3<f32>(0.95), smoothstep(0.006, 0.0, cross) * 0.6);

    return vec4<f32>(colour, 1.0);
}
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_uniform_layout_matches_the_shader() {
        let input = FrameInput {
            time: 1.0,
            stick: [0.5, -0.5],
            look: [0.25, 0.125],
            points: [[0.1, 0.2, 1.0], [0.0, 0.0, 0.0]],
        };
        let bytes = uniform_bytes(&input, (1170, 2532));
        let read =
            |offset: usize| f32::from_ne_bytes(bytes[offset..offset + 4].try_into().unwrap());
        assert_eq!(read(0), 1170.0);
        assert_eq!(read(4), 2532.0);
        assert_eq!(read(8), 1.0);
        assert_eq!(read(12), 1.0, "one active finger");
        assert_eq!(read(16), 0.5);
        assert_eq!(read(20), -0.5);
        assert_eq!(read(24), 0.25);
        assert_eq!(read(28), 0.125);
        assert_eq!(read(32), 0.1);
        assert_eq!(read(40), 1.0);
        assert_eq!(bytes.len(), 64);
    }
}
