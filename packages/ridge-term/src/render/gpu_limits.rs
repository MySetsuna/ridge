//! Device-limit negotiation shared by browser initialization and host tests.

/// Fit requested limits inside one adapter's advertised limits.
///
/// `Limits::using_resolution` only copies texture dimensions. Browser adapters
/// may expose other maxima below wgpu's defaults, so requesting defaults
/// unchanged can reject an otherwise usable device.
pub(super) fn fit_required_limits(
    mut required: wgpu_types::Limits,
    supported: &wgpu_types::Limits,
) -> wgpu_types::Limits {
    macro_rules! clamp_max {
        ($($field:ident),+ $(,)?) => {
            $(required.$field = required.$field.min(supported.$field);)+
        };
    }

    clamp_max!(
        max_texture_dimension_1d,
        max_texture_dimension_2d,
        max_texture_dimension_3d,
        max_texture_array_layers,
        max_bind_groups,
        max_bindings_per_bind_group,
        max_dynamic_uniform_buffers_per_pipeline_layout,
        max_dynamic_storage_buffers_per_pipeline_layout,
        max_sampled_textures_per_shader_stage,
        max_samplers_per_shader_stage,
        max_storage_buffers_per_shader_stage,
        max_storage_textures_per_shader_stage,
        max_uniform_buffers_per_shader_stage,
        max_uniform_buffer_binding_size,
        max_storage_buffer_binding_size,
        max_vertex_buffers,
        max_buffer_size,
        max_vertex_attributes,
        max_vertex_buffer_array_stride,
        max_inter_stage_shader_components,
        max_color_attachments,
        max_color_attachment_bytes_per_sample,
        max_compute_workgroup_storage_size,
        max_compute_invocations_per_workgroup,
        max_compute_workgroup_size_x,
        max_compute_workgroup_size_y,
        max_compute_workgroup_size_z,
        max_compute_workgroups_per_dimension,
        max_push_constant_size,
        max_non_sampler_bindings,
    );
    required.min_uniform_buffer_offset_alignment = required
        .min_uniform_buffer_offset_alignment
        .max(supported.min_uniform_buffer_offset_alignment);
    required.min_storage_buffer_offset_alignment = required
        .min_storage_buffer_offset_alignment
        .max(supported.min_storage_buffer_offset_alignment);
    required
}

pub(super) fn validate_surface_extent(
    width: u32,
    height: u32,
    max_texture_dimension_2d: u32,
) -> Result<(), String> {
    if width <= max_texture_dimension_2d && height <= max_texture_dimension_2d {
        return Ok(());
    }
    Err(format!(
        "surface {width}x{height} exceeds adapter maximum {max_texture_dimension_2d}"
    ))
}

#[cfg(test)]
mod tests {
    use super::{fit_required_limits, validate_surface_extent};

    #[test]
    fn fits_nonstandard_browser_color_attachment_limit() {
        let requested = wgpu_types::Limits::downlevel_defaults();
        let mut supported = requested.clone();
        supported.max_color_attachments = 6;

        let fitted = fit_required_limits(requested, &supported);

        assert_eq!(fitted.max_color_attachments, 6);
        assert!(fitted.check_limits(&supported));
    }

    #[test]
    fn fits_downlevel_maxima_and_adapter_alignments() {
        let requested = wgpu_types::Limits::downlevel_defaults();
        let mut supported = requested.clone();
        supported.max_inter_stage_shader_components = 31;
        supported.max_vertex_buffer_array_stride = 1024;
        supported.min_uniform_buffer_offset_alignment = 512;
        supported.min_storage_buffer_offset_alignment = 512;

        let fitted = fit_required_limits(requested, &supported);

        assert_eq!(fitted.max_inter_stage_shader_components, 31);
        assert_eq!(fitted.max_vertex_buffer_array_stride, 1024);
        assert_eq!(fitted.min_uniform_buffer_offset_alignment, 512);
        assert_eq!(fitted.min_storage_buffer_offset_alignment, 512);
        assert!(fitted.check_limits(&supported));
    }

    #[test]
    fn keeps_adapter_resolution_while_fitting_other_limits() {
        let requested = wgpu_types::Limits::downlevel_webgl2_defaults();
        let mut supported = wgpu_types::Limits::downlevel_defaults();
        supported.max_texture_dimension_2d = 8192;
        supported.max_color_attachments = 6;

        let fitted = fit_required_limits(requested.using_resolution(supported.clone()), &supported);

        assert_eq!(fitted.max_texture_dimension_2d, 8192);
        assert_eq!(fitted.max_color_attachments, 6);
        assert!(fitted.check_limits(&supported));
    }

    #[test]
    fn rejects_surface_extent_before_wgpu_panics() {
        assert!(validate_surface_extent(2048, 2048, 2048).is_ok());
        assert_eq!(
            validate_surface_extent(3100, 1348, 2048).unwrap_err(),
            "surface 3100x1348 exceeds adapter maximum 2048"
        );
    }
}
