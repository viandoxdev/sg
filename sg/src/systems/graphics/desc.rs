// This file is for large descriptors that clutter the screen and/or need to be duplicated

#[macro_export]
macro_rules! geometry_renderpass_desc {
    ($g_buffer:expr) => {
        wgpu::RenderPassDescriptor {
            label: Some("gfx render pass"),
            color_attachments: &[
                Some(wgpu::RenderPassColorAttachment {
                    view: &$g_buffer.albedo_tex,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.0,
                            g: 0.0,
                            b: 0.0,
                            a: 1.0,
                        }),
                        store: true,
                    },
                }),
                Some(wgpu::RenderPassColorAttachment {
                    view: &$g_buffer.position_tex,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.0,
                            g: 0.0,
                            b: 0.0,
                            a: 0.0,
                        }),
                        store: true,
                    },
                }),
                Some(wgpu::RenderPassColorAttachment {
                    view: &$g_buffer.normal_tex,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.0,
                            g: 0.0,
                            b: 0.0,
                            a: 0.0,
                        }),
                        store: true,
                    },
                }),
                Some(wgpu::RenderPassColorAttachment {
                    view: &$g_buffer.mra_tex,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.0,
                            g: 0.0,
                            b: 0.0,
                            a: 0.0,
                        }),
                        store: true,
                    },
                }),
            ],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &$g_buffer.depth_tex,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.0),
                    store: true,
                }),
                stencil_ops: None,
            }),
        }
    };
}

#[macro_export]
macro_rules! shading_renderpass_desc {
    ($view:expr) => {
        wgpu::RenderPassDescriptor {
            label: Some("Shading pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: $view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color {
                        r: 0.0,
                        g: 0.0,
                        b: 0.0,
                        a: 1.0,
                    }),
                    store: true,
                },
            })],
            depth_stencil_attachment: None,
        }
    };
}
#[macro_export]
macro_rules! geometry_pipeline_desc {
    ($layout:expr, $shader:expr) => {
        wgpu::RenderPipelineDescriptor {
            label: Some("Geometry Pipeline"),
            layout: Some($layout),
            vertex: wgpu::VertexState {
                module: $shader,
                entry_point: "vs_main",
                buffers: &[Vertex::desc()],
            },
            fragment: Some(wgpu::FragmentState {
                module: $shader,
                entry_point: "fs_main",
                targets: &[
                    Some(wgpu::ColorTargetState {
                        format: wgpu::TextureFormat::Rgba8UnormSrgb,
                        blend: Some(wgpu::BlendState::REPLACE),
                        write_mask: wgpu::ColorWrites::ALL,
                    }),
                    Some(wgpu::ColorTargetState {
                        format: wgpu::TextureFormat::Rgba16Float,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    }),
                    Some(wgpu::ColorTargetState {
                        format: wgpu::TextureFormat::Rgba16Float,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    }),
                    Some(wgpu::ColorTargetState {
                        format: wgpu::TextureFormat::Rgba8Unorm,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    }),
                ],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: Some(wgpu::Face::Back),
                polygon_mode: wgpu::PolygonMode::Fill,
                unclipped_depth: false,
                conservative: false,
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: TextureManager::DEPTH_FORMAT,
                depth_write_enabled: true,
                depth_compare: wgpu::CompareFunction::Less,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState {
                count: 1,
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            multiview: None,
        }
    };
}

#[macro_export]
macro_rules! shading_pipeline_desc {
    ($layout:expr, $shader:expr, $format:expr) => {
        wgpu::RenderPipelineDescriptor {
            label: Some("Shading pipeline"),
            layout: Some($layout),
            vertex: wgpu::VertexState {
                module: $shader,
                entry_point: "vs_main",
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: $shader,
                entry_point: "fs_main",
                targets: &[Some(wgpu::ColorTargetState {
                    format: $format,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                polygon_mode: wgpu::PolygonMode::Fill,
                unclipped_depth: false,
                conservative: false,
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState {
                count: 1,
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            multiview: None,
        }
    };
}

macro_rules! bind_group_layout_entry {
    (@count $count: tt) => {
        std::num::NonZeroU32::new($count)
    };
    (@count) => {
        None
    };

    (@key texture, view_dim: $vd:ident ) => { wgpu::TextureViewDimension::$vd };
    (@key texture, sample: Float ) => { wgpu::TextureSampleType::Float { filterable: false } };
    (@key texture, sample: FloatFilterable ) => { wgpu::TextureSampleType::Float { filterable: true } };
    (@key texture, sample: $v:ident ) => { wgpu::TextureSampleType::$v };
    (@key texture, multisampled: $v:expr ) => { $v };

    (@key buffer, type: Uniform) => { wgpu::BufferBindingType::Uniform };
    (@key buffer, type: Storage) => { wgpu::BufferBindingType::Storage { read_only: false } };
    (@key buffer, type: ReadOnlyStorage) => { wgpu::BufferBindingType::Storage { read_only: true } };
    (@key buffer, dyn_off: $b:expr) => { $b };
    (@key buffer, min_size: $b:expr) => { $b };

    (@key storage_texture, access: $a:ident) => { wgpu::StorageTextureAccess::$a };
    (@key storage_texture, format: $f:ident) => { wgpu::TextureFormat::$f };

    (@key $i:ident, $k:ident: $v:tt) => { unreachable!() };

    (@pick(value) $id:ident, $key:ident, $k:ident: $v:tt) => {{
        macro_rules! _m {
            ($key, $key) => { bind_group_layout_entry!(@key $id, $key: $v) };
            ($key, $k) => { panic!() };
        }
        _m!($key, $k)
    }};

    (@pick(is) $id:ident, $key:ident, $k:ident) => {{
        macro_rules! _m {
            ($key, $key) => { true };
            ($key, $k) => { false };
        }
        _m!($key, $k)
    }};
    (@pick $id:ident, $key:ident, $($k:ident: $v:tt),*) => {{
        $(
            if bind_group_layout_entry!(@pick(is) $id, $key, $k) {
                bind_group_layout_entry!(@pick(value) $id, $key, $k: $v)
            } else
        )*
        {
            unreachable!()
        }
    }};
    (@pick_default $id:ident, $key:ident, $($k:ident: $v:tt),*) => {{
        $(
            if bind_group_layout_entry!(@pick(is) $id, $key, $k) {
                bind_group_layout_entry!(@pick(value) $id, $key, $k: $v)
            } else
        )*
        {
            Default::default()
        }
    }};
    (@pick_or $id:ident, $key:ident: $d:expr, $($k:ident: $v:tt),*) => {{
        $(
            if bind_group_layout_entry!(@pick(is) $id, $key, $k) {
                bind_group_layout_entry!(@pick(value) $id, $key, $k: $v)
            } else
        )*
        {
            $d
        }
    }};

    (@type Buffer($($k:ident: $v:tt),*$(,)?)) => {
        wgpu::BindingType::Buffer {
            ty:                 bind_group_layout_entry!(@pick         buffer, type,     $($k: $v),*),
            has_dynamic_offset: bind_group_layout_entry!(@pick_default buffer, dyn_off,  $($k: $v),*),
            min_binding_size:   bind_group_layout_entry!(@pick_default buffer, min_size, $($k: $v),*),
        }
    };
    (@type Sampler($ty:ident)) => {
        wgpu::BindingType::Sampler(wgpu::SamplerBindingType::$ty)
    };
    (@type Texture($($k:ident: $v:tt),*$(,)?)) => {
        wgpu::BindingType::Texture {
            view_dimension: bind_group_layout_entry!(@pick         texture, view_dim,     $($k: $v),*),
            sample_type:    bind_group_layout_entry!(@pick         texture, sample,       $($k: $v),*),
            multisampled:   bind_group_layout_entry!(@pick_default texture, multisampled, $($k: $v),*),
        }
    };
    (@type StorageTexture($($k:ident: $v:tt),*$(,)?)) => {
        wgpu::BindingType::StorageTexture {
            access: bind_group_layout_entry!(@pick storage_texture, access,   $($k: $v),*),
            format: bind_group_layout_entry!(@pick storage_texture, format,   $($k: $v),*),
            view_dimension: bind_group_layout_entry!(@pick texture, view_dim, $($k: $v),*),
        }
    };
    (@type $t:ident($($cont:tt)*)) => { compile_error!("Unknown type") };

    ($binding:literal => $($vis:ident),* | $type:ident($($cont:tt)*)$([$count:expr])?) => {
        wgpu::BindGroupLayoutEntry {
            count: bind_group_layout_entry!(@count $($count)?),
            binding: $binding,
            visibility: wgpu::ShaderStages::empty() $( | wgpu::ShaderStages::$vis)*,
            ty: bind_group_layout_entry!(@type $type($($cont)*)),
        }
    };
}

macro_rules! bind_group_entry {
    (@pick(value) $id:ident, $key:ident, $k:ident: $v:tt) => {{
        macro_rules! _m {
            ($key, $key) => { bind_group_entry!(@key $id, $key: $v) };
            ($key, $k) => { panic!() };
        }
        _m!($key, $k)
    }};

    (@pick(is) $id:ident, $key:ident, $k:ident) => {{
        macro_rules! _m {
            ($key, $key) => { true };
            ($key, $k) => { false };
        }
        _m!($key, $k)
    }};
    (@pick $id:ident, $key:ident, $($k:ident: $v:tt),*) => {{
        $(
            if bind_group_entry!(@pick(is) $id, $key, $k) {
                bind_group_entry!(@pick(value) $id, $key, $k: $v)
            } else
        )*
        {
            unreachable!()
        }
    }};
    (@pick_default $id:ident, $key:ident, $($k:ident: $v:tt),*) => {{
        $(
            if bind_group_entry!(@pick(is) $id, $key, $k) {
                bind_group_entry!(@pick(value) $id, $key, $k: $v)
            } else
        )*
        {
            Default::default()
        }
    }};
    (@pick_or $id:ident, $key:ident: $d:expr, $($k:ident: $v:tt),*) => {{
        $(
            if bind_group_entry!(@pick(is) $id, $key, $k) {
                bind_group_entry!(@pick(value) $id, $key, $k: $v)
            } else
        )*
        {
            $d
        }
    }};

    (@key any, $i:ident: $v:tt) => { $v };
    (@key $i:ident, $k:ident: $v:tt) => { compile_error!("Unknown key") };
    
    (@type Buffer($($k:ident: $v:tt),*$(,)?)) => {
        wgpu::BindingResource::Buffer(wgpu::BufferBinding {
            buffer: bind_group_entry!(@pick         any, buffer, $($k: $v),*),
            offset: bind_group_entry!(@pick_default any, offset, $($k: $v),*),
            size:   bind_group_entry!(@pick_default any, size,   $($k: $v),*),
        })
    };
    (@type BufferArray($({$($k:ident: $v:tt),*$(,)?}),*)) => {
        wgpu::BindingResource::BufferArray(&[$(
            wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                buffer: bind_group_entry!(@pick         any, buffer, $($k: $v),*),
                offset: bind_group_entry!(@pick_default any, offset, $($k: $v),*),
                size:   bind_group_entry!(@pick_default any, size,   $($k: $v),*),
            })
        ),*])
    };
    (@type Sampler($s:expr)) => {
        wgpu::BindingResource::Sampler($s)
    };
    (@type SamplerArray($($s:expr),*)) => {
        wgpu::BindingResource::SamplerArray(&[$($s),*])
    };
    (@type TextureView($t:expr)) => {
        wgpu::BindingResource::TextureView($t)
    };
    (@type TextureViewArray($($t:expr),*)) => {
        wgpu::BindingResource::TextureView(&[$($t),*])
    };
    (@type $t:ident($($cont:tt)*)) => { compile_error!("Unknown type") };

    ($binding:literal | $type:ident($($cont:tt)*)) => {
        wgpu::BindGroupEntry {
            binding: $binding,
            resource: bind_group_entry!(@type $type($($cont)*)),
        }
    };
}
macro_rules! create_bind_group_layout {
    (
        $device:expr,
        $label:literal: {
            $($binding:literal => $($vis:ident),* | $type:ident($($cont:tt)*)$([$count:expr])?),*$(,)?
        }
    ) => {
        $device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            entries: &[$(
                bind_group_layout_entry!($binding => $($vis),* | $type($($cont)*)$([$count])?)
            ),*],
            label: Some($label)
        })
    };
    (
        $device:expr,
        {
            $($binding:literal => $($vis:ident),* | $type:ident($($cont:tt)*)$([$count:expr])?),*$(,)?
        }
    ) => {
        $device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            entries: &[$(
                bind_group_layout_entry!($binding => $($vis),* | $type($($cont)*)$([$count])?)
            ),*],
            label: None,
        })
    };
}
macro_rules! create_bind_group {
    (
        $device:expr,
        $layout:expr,
        $label:literal: {
            $($binding:literal | $type:ident($($cont:tt)*)),*$(,)?
        }
    ) => {
        $device.create_bind_group(&wgpu::BindGroupDescriptor {
            entries: &[$(
                bind_group_entry!($binding | $type($($cont)*))
            ),*],
            label: Some($label),
            layout: $layout,
        })
    };
    (
        $device:expr,
        $layout:expr,
        {
            $($binding:literal | $type:ident($($cont:tt)*)),*$(,)?
        }
    ) => {
        $device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            entries: &[$(
                bind_group_entry!($binding | $type($($cont)*))
            ),*],
            label: None,
            layout: $layout,
        })
    };
}
