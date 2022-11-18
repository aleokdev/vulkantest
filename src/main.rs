use std::ffi::CStr;

use anyhow::anyhow;
use ash::vk;
use raw_window_handle::{HasRawDisplayHandle, HasRawWindowHandle};

const VK_KHR_SURFACE: &CStr = cstr::cstr!("VK_KHR_surface");
const VK_EXT_DEBUG_UTILS: &CStr = cstr::cstr!("VK_EXT_debug_utils");
const VK_LAYER_KHRONOS_VALIDATION: &CStr = cstr::cstr!("VK_LAYER_KHRONOS_validation");
const VK_KHR_SWAPCHAIN: &CStr = cstr::cstr!("VK_KHR_swapchain");

fn main() -> anyhow::Result<()> {
    env_logger::init();

    let result = run();

    if let Err(err) = &result {
        log::error!("Fatal error: {}", err);
    }

    result
}

fn run() -> anyhow::Result<()> {
    let event_loop = winit::event_loop::EventLoop::new();
    let window = winit::window::WindowBuilder::new()
        .with_inner_size(winit::dpi::PhysicalSize::new(1080, 720))
        .with_title("Vulkan Test")
        .build(&event_loop)?;

    let entry = unsafe {
        ash::Entry::load_from(
            std::env::var("VK_LIBRARY_PATH")
                .or_else(|err| Err(anyhow!("Error getting VK_LIBRARY_PATH env variable: {err}")))?,
        )?
    };
    let required_instance_extensions =
        ash_window::enumerate_required_extensions(window.raw_display_handle())?;
    let instance_extensions = [
        required_instance_extensions,
        &[VK_KHR_SURFACE.as_ptr(), VK_EXT_DEBUG_UTILS.as_ptr()],
    ]
    .concat();
    let layers = &[VK_LAYER_KHRONOS_VALIDATION.as_ptr()];

    let instance = unsafe {
        entry.create_instance(
            &vk::InstanceCreateInfo::builder()
                .application_info(
                    &vk::ApplicationInfo::builder()
                        .application_name(cstr::cstr!("Vulkan test"))
                        .api_version(vk::API_VERSION_1_1),
                )
                .enabled_extension_names(&instance_extensions)
                .enabled_layer_names(layers),
            None,
        )?
    };

    let debug_utils_ext = ash::extensions::ext::DebugUtils::new(&entry, &instance);
    unsafe extern "system" fn logger(
        severity: vk::DebugUtilsMessageSeverityFlagsEXT,
        _ty: vk::DebugUtilsMessageTypeFlagsEXT,
        data: *const vk::DebugUtilsMessengerCallbackDataEXT,
        _: *mut std::os::raw::c_void,
    ) -> vk::Bool32 {
        let msg = CStr::from_ptr((*data).p_message).to_string_lossy();
        match severity {
            vk::DebugUtilsMessageSeverityFlagsEXT::VERBOSE => {
                log::debug!("{}", msg)
            }
            vk::DebugUtilsMessageSeverityFlagsEXT::INFO => {
                log::info!("{}", msg)
            }
            vk::DebugUtilsMessageSeverityFlagsEXT::ERROR => {
                log::error!("{}", msg)
            }
            vk::DebugUtilsMessageSeverityFlagsEXT::WARNING => {
                log::warn!("{}", msg)
            }
            _ => (),
        }
        vk::FALSE
    }
    let debug_messenger = unsafe {
        debug_utils_ext.create_debug_utils_messenger(
            &vk::DebugUtilsMessengerCreateInfoEXT::builder()
                .message_severity(
                    vk::DebugUtilsMessageSeverityFlagsEXT::VERBOSE
                        | vk::DebugUtilsMessageSeverityFlagsEXT::INFO
                        | vk::DebugUtilsMessageSeverityFlagsEXT::ERROR
                        | vk::DebugUtilsMessageSeverityFlagsEXT::WARNING,
                )
                .message_type(
                    vk::DebugUtilsMessageTypeFlagsEXT::GENERAL
                        | vk::DebugUtilsMessageTypeFlagsEXT::PERFORMANCE
                        | vk::DebugUtilsMessageTypeFlagsEXT::VALIDATION,
                )
                .pfn_user_callback(Some(logger)),
            None,
        )?
    };

    let surface_ext = ash::extensions::khr::Surface::new(&entry, &instance);
    let surface = unsafe {
        ash_window::create_surface(
            &entry,
            &instance,
            window.raw_display_handle(),
            window.raw_window_handle(),
            None,
        )?
    };

    let (physical_device, physical_device_properties) = unsafe {
        instance
            .enumerate_physical_devices()?
            .into_iter()
            .map(|device| (device, instance.get_physical_device_properties(device)))
            .find(|(device, properties)| {
                properties.api_version >= vk::API_VERSION_1_1
                    && surface_ext
                        .get_physical_device_surface_formats(*device, surface)
                        .map(|formats| !formats.is_empty())
                        .unwrap_or(false)
                    && surface_ext
                        .get_physical_device_surface_present_modes(*device, surface)
                        .map(|formats| !formats.is_empty())
                        .unwrap_or(false)
            })
    }
    .ok_or_else(|| anyhow!("Could not find suitable physical device"))?;

    println!("Found suitable physical device: {}", unsafe {
        CStr::from_ptr(physical_device_properties.device_name.as_ptr()).to_string_lossy()
    });

    let queue_family_properties =
        unsafe { instance.get_physical_device_queue_family_properties(physical_device) };

    let queue_family_index = queue_family_properties
        .iter()
        .enumerate()
        .find_map(|(idx, properties)| {
            properties
                .queue_flags
                .contains(vk::QueueFlags::GRAPHICS)
                .then(|| idx)
        })
        .ok_or_else(|| anyhow::anyhow!("Could not find graphics queue in GPU chosen"))?
        as u32;

    let device = unsafe {
        instance.create_device(
            physical_device,
            &vk::DeviceCreateInfo::builder()
                .queue_create_infos(&[vk::DeviceQueueCreateInfo::builder()
                    .queue_family_index(queue_family_index as u32)
                    .queue_priorities(&[1.0f32])
                    .build()])
                .enabled_extension_names(&[VK_KHR_SWAPCHAIN.as_ptr()]),
            None,
        )
    }?;

    let min_image_count = unsafe {
        surface_ext
            .get_physical_device_surface_capabilities(physical_device, surface)?
            .min_image_count
    };
    let surface_formats =
        unsafe { surface_ext.get_physical_device_surface_formats(physical_device, surface) }?;
    log::info!("Formats available:");
    for format in &surface_formats {
        log::info!(
            "Color space: {:?}, Format: {:?}",
            format.color_space,
            format.format
        );
    }
    let surface_format = surface_formats
        .into_iter()
        .find(|format| format.color_space == vk::ColorSpaceKHR::SRGB_NONLINEAR).ok_or_else(|| anyhow::anyhow!("Could not find any format supported by the surface with the SRGB_NONLINEAR color space"))?;

    let swapchain_ext = ash::extensions::khr::Swapchain::new(&instance, &device);
    let swapchain = unsafe {
        swapchain_ext.create_swapchain(
            &vk::SwapchainCreateInfoKHR::builder()
                .clipped(true)
                .composite_alpha(vk::CompositeAlphaFlagsKHR::OPAQUE) // TODO: Use POST_MULTIPLIED when available
                .flags(vk::SwapchainCreateFlagsKHR::default())
                .image_array_layers(1)
                .image_color_space(surface_format.color_space)
                .image_extent(vk::Extent2D {
                    width: window.inner_size().width,
                    height: window.inner_size().height,
                })
                .image_format(surface_format.format)
                .image_sharing_mode(vk::SharingMode::EXCLUSIVE) // Only one queue can access the swapchain at a time
                .image_usage(vk::ImageUsageFlags::COLOR_ATTACHMENT)
                .min_image_count(min_image_count)
                .old_swapchain(vk::SwapchainKHR::null())
                .pre_transform(vk::SurfaceTransformFlagsKHR::IDENTITY)
                .present_mode(vk::PresentModeKHR::FIFO)
                .queue_family_indices(&[queue_family_index])
                .surface(surface),
            None,
        )?
    };

    let command_pool = unsafe {
        device.create_command_pool(
            &vk::CommandPoolCreateInfo::builder()
                .queue_family_index(queue_family_index)
                .flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER), // Allow resetting command buffers individually
            None,
        )?
    };

    let command_buffer = unsafe {
        device.allocate_command_buffers(
            &vk::CommandBufferAllocateInfo::builder()
                .command_buffer_count(1)
                .command_pool(command_pool)
                .level(vk::CommandBufferLevel::PRIMARY),
        )?
    };

    let color_attachment = vk::AttachmentDescription::builder()
        .format(surface_format.format)
        .initial_layout(vk::ImageLayout::UNDEFINED)
        .final_layout(vk::ImageLayout::PRESENT_SRC_KHR)
        .samples(vk::SampleCountFlags::TYPE_1)
        .stencil_load_op(vk::AttachmentLoadOp::DONT_CARE)
        .stencil_store_op(vk::AttachmentStoreOp::DONT_CARE)
        .store_op(vk::AttachmentStoreOp::STORE)
        .load_op(vk::AttachmentLoadOp::CLEAR);

    let attachment_ref = &[vk::AttachmentReference::builder()
        .attachment(0)
        .layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
        .build()];

    let subpass = vk::SubpassDescription::builder()
        .color_attachments(attachment_ref)
        .pipeline_bind_point(vk::PipelineBindPoint::GRAPHICS);

    let render_pass = unsafe {
        device.create_render_pass(
            &vk::RenderPassCreateInfo::builder()
                .attachments(&[color_attachment.build()])
                .subpasses(&[subpass.build()]),
            None,
        )?
    };

    unsafe {
        device.destroy_render_pass(render_pass, None);
        device.destroy_command_pool(command_pool, None);
        swapchain_ext.destroy_swapchain(swapchain, None);
        device.destroy_device(None);
        surface_ext.destroy_surface(surface, None);
        debug_utils_ext.destroy_debug_utils_messenger(debug_messenger, None);
        instance.destroy_instance(None);
    }

    Ok(())
}
