from orders_service import BaseRepository, ExportedClient
from orders_service import ExportedClient as ClientAlias
from orders_service.conditional import ConditionalClient
from orders_service.conditional_single import ExportedClient as ConditionalSingleClient
from orders_service.paths import ExportedPath
import orders_service.nested.facade as nested_facade
import orders_service.nested.models as nested_models
from orders_service.nested.facade import _private_helper, package_helper
from orders_service.empty_facade import HiddenClient
from orders_service.dynamic_facade import DynamicClient
from orders_service.module_export import client_module
from orders_service.module_export import pathlib as pathlib_module
from orders_service.external_shadow import Client as ShadowedClient
from orders_service.conditional_all_facade import ConditionalExport
from orders_service.explicit_d import DeepExplicitClient


def open_report_session() -> str:
	repo = BaseRepository("report-dsn")
	return repo.open_session()


def open_exported_client() -> str:
    client = ExportedClient("report")
    return client.label()


def create_exported_client() -> str:
	return ExportedClient.create("report").label()


def open_exported_path() -> str:
    return str(ExportedPath("report.txt"))


def open_conditional_client() -> str:
    return ConditionalClient("report").label()


def open_single_conditional_client() -> str:
	return ConditionalSingleClient("report").label()


def open_aliased_client() -> str:
    return ClientAlias("alias").label()


def open_nested_facade_client() -> str:
    return nested_facade.ExportedClient("nested").label()


def open_nested_wildcard_field() -> str:
	return nested_models.NestedField("nested").name


def call_package_helper() -> str:
	return package_helper("package")


def call_private_package_helper() -> str:
	return _private_helper("private")


def call_hidden_client():
	return HiddenClient()


def call_dynamic_client():
	return DynamicClient()


def call_exported_module_client() -> str:
	return client_module.ExportedClient("module").label()


def call_exported_external_module():
	return pathlib_module.Path("external.txt")


def call_shadowed_client():
	return ShadowedClient("shadowed")


def call_function_conditional_client(enabled):
	if enabled:
		from orders_service.wildcard_impl import ExportedClient as FunctionConditionalClient
	return FunctionConditionalClient("conditional")


def call_multi_function_conditional_client(enabled):
	if enabled:
		from orders_service.conditional_a import ConditionalClient as RuntimeClient
	else:
		from orders_service.conditional_b import ConditionalClient as RuntimeClient
	return RuntimeClient("conditional")


def configure_scoped_client():
	from orders_service.wildcard_impl import ExportedClient as ScopedClient
	return ScopedClient("scoped")


def call_leaked_scoped_client():
	return ScopedClient("leaked")


def call_conditional_export():
	return ConditionalExport()


def call_deep_explicit_client():
	return DeepExplicitClient()
