use core::ffi::CStr;

use pgrx::pg_sys;

pub(crate) fn resolve_type_oid(type_name: &str) -> pg_sys::Oid {
	unsafe {
		let ext_oid = pg_sys::get_extension_oid(c"pg_code_moniker".as_ptr(), false);
		let nsp_oid = pg_sys::get_extension_schema(ext_oid);
		let nsp_name = pg_sys::get_namespace_name(nsp_oid);
		let nsp_str = CStr::from_ptr(nsp_name)
			.to_str()
			.expect("namespace name must be UTF-8");
		::pgrx::wrappers::regtypein(&format!("{nsp_str}.{type_name}"))
	}
}
