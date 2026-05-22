use std::ffi::CStr;
use std::os::raw::{c_int, c_void, c_char};

use ffmpeg_sys_next as sys;

unsafe extern "C" fn ffmpeg_log_callback(
	_avcl: *mut c_void,
	level: c_int,
	fmt: *const c_char,
	vl: *mut i8, // va_list (opaque)
) {
	if level > sys::av_log_get_level() { return; }

	// FFmpeg provides a helper to format log messages:
	// int av_log_format_line(void*, int, const char*, va_list, char*, int, int*)
	let mut line = [0i8; 1024];
	let mut print_prefix: c_int = 0;

	sys::av_log_format_line(
		_avcl,
		level,
		fmt,
		vl,
		line.as_mut_ptr(),
		line.len() as c_int,
		&mut print_prefix,
	);

	if let Ok(msg) = CStr::from_ptr(line.as_ptr()).to_str() {
		match level {
			sys::AV_LOG_TRACE => tracing::trace!(target: "ffmpeg", "{msg}"),
			sys::AV_LOG_VERBOSE | sys::AV_LOG_DEBUG => tracing::debug!(target: "ffmpeg", "{msg}"),
			sys::AV_LOG_INFO => tracing::info!(target: "ffmpeg", "{msg}"),
			sys::AV_LOG_WARNING => tracing::warn!(target: "ffmpeg", "{msg}"),
			sys::AV_LOG_PANIC | sys::AV_LOG_FATAL | sys::AV_LOG_ERROR => tracing::error!(target: "ffmpeg", "{msg}"),
			_ => unreachable!(),
		}
	}
}

pub fn install() {
	unsafe {
		sys::av_log_set_callback(Some(ffmpeg_log_callback));
	}
}
