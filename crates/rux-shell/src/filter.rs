//! `filter` and `backdrop-filter` on the GPU.
//!
//! vello draws vectors and has no filters, so a filtered part of the frame
//! is drawn apart ([`rux_paint::Offscreen`]): rendered by vello into a
//! texture of its own, put through each step here as a compute pass, and
//! bound to the image the frame's scene holds in its place, all before the
//! frame itself is drawn. The steps are a 4x5 colour matrix, a separable
//! Gaussian blur, and `drop-shadow()`, which is the alpha offset, tinted,
//! blurred and drawn under. Every pass reads straight colour, which is what
//! vello writes, and works in premultiplied colour where pixels are mixed.

use rux_layout::filter::FilterOp;
use rux_paint::Offscreen;
use vello::kurbo::Affine;
use vello::peniko::{Color, ImageData};
use vello::wgpu;
use vello::{AaConfig, Renderer, RenderParams, Scene};

const SHADER: &str = r#"
struct Params {
    m0: vec4<f32>,
    m1: vec4<f32>,
    m2: vec4<f32>,
    m3: vec4<f32>,
    moff: vec4<f32>,
    color: vec4<f32>,
    dir: vec2<i32>,
    offset: vec2<i32>,
    sigma: f32,
    radius: i32,
    step: i32,
    pad: i32,
}

@group(0) @binding(0) var src: texture_2d<f32>;
@group(0) @binding(1) var dst: texture_storage_2d<rgba8unorm, write>;
@group(0) @binding(2) var<uniform> p: Params;
@group(0) @binding(3) var below: texture_2d<f32>;

// Premultiplied, and transparent outside the texture.
fn load(t: texture_2d<f32>, c: vec2<i32>) -> vec4<f32> {
    let size = vec2<i32>(textureDimensions(t));
    if (any(c < vec2<i32>(0)) || any(c >= size)) {
        return vec4<f32>(0.0);
    }
    let v = textureLoad(t, c, 0);
    return vec4<f32>(v.rgb * v.a, v.a);
}

// Straight again, as vello reads an image.
fn store(c: vec2<i32>, v: vec4<f32>) {
    var rgb = vec3<f32>(0.0);
    if (v.a > 0.0) {
        rgb = v.rgb / v.a;
    }
    textureStore(dst, c, vec4<f32>(clamp(rgb, vec3<f32>(0.0), vec3<f32>(1.0)), clamp(v.a, 0.0, 1.0)));
}

fn inside(c: vec2<i32>) -> bool {
    let size = vec2<i32>(textureDimensions(src));
    return all(c < size);
}

@compute @workgroup_size(8, 8)
fn matrix(@builtin(global_invocation_id) id: vec3<u32>) {
    let c = vec2<i32>(id.xy);
    if (!inside(c)) { return; }
    let v = textureLoad(src, c, 0);
    let o = vec4<f32>(dot(p.m0, v), dot(p.m1, v), dot(p.m2, v), dot(p.m3, v)) + p.moff;
    textureStore(dst, c, clamp(o, vec4<f32>(0.0), vec4<f32>(1.0)));
}

@compute @workgroup_size(8, 8)
fn blur(@builtin(global_invocation_id) id: vec3<u32>) {
    let c = vec2<i32>(id.xy);
    if (!inside(c)) { return; }
    var sum = vec4<f32>(0.0);
    var weights = 0.0;
    for (var k = -p.radius; k <= p.radius; k = k + p.step) {
        let w = exp(-f32(k * k) / (2.0 * p.sigma * p.sigma));
        sum = sum + load(src, c + p.dir * k) * w;
        weights = weights + w;
    }
    store(c, sum / weights);
}

// The shape a drop shadow casts: the alpha, moved by the offset, in the
// shadow's colour.
@compute @workgroup_size(8, 8)
fn silhouette(@builtin(global_invocation_id) id: vec3<u32>) {
    let c = vec2<i32>(id.xy);
    if (!inside(c)) { return; }
    store(c, p.color * load(src, c - p.offset).a);
}

// `src` over `below`.
@compute @workgroup_size(8, 8)
fn over(@builtin(global_invocation_id) id: vec3<u32>) {
    let c = vec2<i32>(id.xy);
    if (!inside(c)) { return; }
    let top = load(src, c);
    store(c, top + load(below, c) * (1.0 - top.a));
}
"#;

#[repr(C)]
#[derive(Clone, Copy)]
struct Params {
    m: [[f32; 4]; 5],
    color: [f32; 4],
    dir: [i32; 2],
    offset: [i32; 2],
    sigma: f32,
    radius: i32,
    step: i32,
    pad: i32,
}

impl Params {
    fn zero() -> Self {
        Self { m: [[0.0; 4]; 5], color: [0.0; 4], dir: [0, 0], offset: [0, 0], sigma: 1.0, radius: 0, step: 1, pad: 0 }
    }

    fn bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(128);
        for row in &self.m {
            row.iter().for_each(|v| out.extend_from_slice(&v.to_le_bytes()));
        }
        self.color.iter().for_each(|v| out.extend_from_slice(&v.to_le_bytes()));
        self.dir.iter().chain(&self.offset).for_each(|v| out.extend_from_slice(&v.to_le_bytes()));
        out.extend_from_slice(&self.sigma.to_le_bytes());
        for v in [self.radius, self.step, self.pad] {
            out.extend_from_slice(&v.to_le_bytes());
        }
        out
    }
}

/// The compute pipelines, made once per device, and the textures the
/// passes run through, kept from frame to frame.
pub(crate) struct GpuFilters {
    layout: wgpu::BindGroupLayout,
    matrix: wgpu::ComputePipeline,
    blur: wgpu::ComputePipeline,
    silhouette: wgpu::ComputePipeline,
    over: wgpu::ComputePipeline,
    /// Textures free for this frame, and those taken by it.
    free: Vec<wgpu::Texture>,
    taken: Vec<wgpu::Texture>,
    /// The images bound this frame, let go of once it is drawn.
    bound: Vec<ImageData>,
}

impl GpuFilters {
    pub(crate) fn new(device: &wgpu::Device) -> Self {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("rux: filters"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let texture = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: false },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("rux: filter bindings"),
            entries: &[
                texture(0),
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::StorageTexture {
                        access: wgpu::StorageTextureAccess::WriteOnly,
                        format: wgpu::TextureFormat::Rgba8Unorm,
                        view_dimension: wgpu::TextureViewDimension::D2,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                texture(3),
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("rux: filters"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = |entry: &str| {
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some(entry),
                layout: Some(&pipeline_layout),
                module: &module,
                entry_point: Some(entry),
                compilation_options: Default::default(),
                cache: None,
            })
        };
        Self {
            matrix: pipeline("matrix"),
            blur: pipeline("blur"),
            silhouette: pipeline("silhouette"),
            over: pipeline("over"),
            layout,
            free: Vec::new(),
            taken: Vec::new(),
            bound: Vec::new(),
        }
    }

    /// A texture of this size for this frame, reused when one is free.
    fn take(&mut self, device: &wgpu::Device, width: u32, height: u32) -> wgpu::Texture {
        let at = self.free.iter().position(|t| t.width() == width && t.height() == height);
        let texture = match at {
            Some(i) => self.free.swap_remove(i),
            None => device.create_texture(&wgpu::TextureDescriptor {
                label: Some("rux: filtered"),
                size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::STORAGE_BINDING
                    | wgpu::TextureUsages::TEXTURE_BINDING
                    | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            }),
        };
        self.taken.push(texture.clone());
        texture
    }

    /// One pass, `src` (and `below`) into `dst`.
    #[allow(clippy::too_many_arguments)]
    fn pass(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        pipeline: &wgpu::ComputePipeline,
        src: &wgpu::Texture,
        below: &wgpu::Texture,
        dst: &wgpu::Texture,
        params: Params,
    ) {
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("rux: filter params"),
            size: 128,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mut bytes = params.bytes();
        bytes.resize(128, 0);
        queue.write_buffer(&buffer, 0, &bytes);
        let view = |t: &wgpu::Texture| t.create_view(&wgpu::TextureViewDescriptor::default());
        let (src_view, dst_view, below_view) = (view(src), view(dst), view(below));
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("rux: filter pass"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&src_view) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&dst_view) },
                wgpu::BindGroupEntry { binding: 2, resource: buffer.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(&below_view) },
            ],
        });
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("rux: filter") });
        {
            let mut cpass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor { label: None, timestamp_writes: None });
            cpass.set_pipeline(pipeline);
            cpass.set_bind_group(0, &group, &[]);
            cpass.dispatch_workgroups(dst.width().div_ceil(8), dst.height().div_ceil(8), 1);
        }
        queue.submit([encoder.finish()]);
    }

    /// A Gaussian blur of `sigma` device pixels, horizontal then vertical,
    /// from `src` into a new texture.
    fn blur(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, src: &wgpu::Texture, sigma: f32) -> wgpu::Texture {
        let (w, h) = (src.width(), src.height());
        if sigma < 0.1 {
            return src.clone();
        }
        // Three deviations reach almost all of the curve; past 64 taps a
        // side, every other tap or more is skipped, which the curve's
        // smoothness hides.
        let radius = (3.0 * sigma).ceil() as i32;
        let step = (radius / 64).max(1);
        let mut params = Params::zero();
        params.sigma = sigma;
        params.radius = radius;
        params.step = step;
        let across = self.take(device, w, h);
        params.dir = [1, 0];
        self.pass(device, queue, &self.blur, src, src, &across, params);
        let down = self.take(device, w, h);
        params.dir = [0, 1];
        self.pass(device, queue, &self.blur, &across, &across, &down, params);
        down
    }

    /// Draws each part apart and binds the result to its image, so the
    /// frame's scene finds it. `scale` is device pixels per logical pixel,
    /// and `background` what a backdrop is drawn over.
    pub(crate) fn run(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        renderer: &mut Renderer,
        jobs: &[Offscreen],
        scale: f64,
        background: Color,
    ) {
        for job in jobs {
            let (w, h) = (job.image.width, job.image.height);
            let mut scaled = Scene::new();
            scaled.append(&job.scene, Some(Affine::scale(scale)));
            let drawn = self.take(device, w, h);
            let view = drawn.create_view(&wgpu::TextureViewDescriptor::default());
            let base = if job.backdrop { background } else { Color::TRANSPARENT };
            let params = RenderParams { base_color: base, width: w, height: h, antialiasing_method: AaConfig::Area };
            if renderer.render_to_texture(device, queue, &scaled, &view, &params).is_err() {
                continue;
            }
            let mut now = drawn;
            for op in job.ops.iter() {
                now = match op {
                    FilterOp::Matrix(m) => {
                        let mut params = Params::zero();
                        for (r, row) in params.m.iter_mut().take(4).enumerate() {
                            row.copy_from_slice(&m[r * 5..r * 5 + 4]);
                        }
                        params.m[4] = [m[4], m[9], m[14], m[19]];
                        let out = self.take(device, w, h);
                        self.pass(device, queue, &self.matrix, &now, &now, &out, params);
                        out
                    }
                    FilterOp::Blur(sigma) => self.blur(device, queue, &now, sigma * scale as f32),
                    FilterOp::DropShadow { dx, dy, blur, color } => {
                        let mut params = Params::zero();
                        params.offset = [(dx * scale as f32).round() as i32, (dy * scale as f32).round() as i32];
                        params.color = [color.r * color.a, color.g * color.a, color.b * color.a, color.a];
                        let cast = self.take(device, w, h);
                        self.pass(device, queue, &self.silhouette, &now, &now, &cast, params);
                        // A shadow's blur is a radius, twice its deviation.
                        let soft = self.blur(device, queue, &cast, blur * scale as f32 / 2.0);
                        let out = self.take(device, w, h);
                        self.pass(device, queue, &self.over, &now, &soft, &out, Params::zero());
                        out
                    }
                };
            }
            let base = wgpu::TexelCopyTextureInfoBase {
                texture: now,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            };
            renderer.override_image(&job.image, Some(base));
            self.bound.push(job.image.clone());
        }
    }

    /// Once the frame is drawn: its images are let go of, and its textures
    /// are free for the next one.
    pub(crate) fn finish(&mut self, renderer: &mut Renderer) {
        for image in self.bound.drain(..) {
            renderer.override_image(&image, None);
        }
        self.free.append(&mut self.taken);
        // A resize leaves textures of a size no frame asks for again.
        if self.free.len() > 24 {
            self.free.drain(..self.free.len() - 24);
        }
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use vello::wgpu::naga;

    /// The shader is only compiled when a frame first has a filter, so a
    /// mistake in it would otherwise surface as a crash in a window: `cast`,
    /// a reserved word in WGSL, did.
    #[test]
    fn the_shader_compiles_and_has_every_pass() {
        let module = naga::front::wgsl::parse_str(super::SHADER).unwrap_or_else(|e| panic!("{}", e.emit_to_string(super::SHADER)));
        naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::empty())
            .validate(&module)
            .expect("the shader validates");
        let names: Vec<&str> = module.entry_points.iter().map(|e| e.name.as_str()).collect();
        for pass in ["matrix", "blur", "silhouette", "over"] {
            assert!(names.contains(&pass), "no `{pass}` in {names:?}");
        }
        // The uniform is 128 bytes, as `Params::bytes` writes it.
        let params = module.types.iter().find(|(_, t)| t.name.as_deref() == Some("Params")).expect("Params");
        assert_eq!(params.1.inner.size(module.to_ctx()), 128);
        assert_eq!(super::Params::zero().bytes().len(), 128);
    }
}
