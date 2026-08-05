use public_api::api::{AliasedModel, DeepModel, SharedModel};
use public_api::facade::ExternalModel;
use public_api::model_facade::NestedModel;

mod local_facade {
	pub(crate) use public_api::api::SharedModel;
}

pub fn shared_model() -> SharedModel {
	SharedModel
}

pub fn aliased_model() -> AliasedModel {
	AliasedModel
}

pub fn deep_model() -> DeepModel {
	DeepModel
}

pub fn external_model() -> ExternalModel {
	ExternalModel
}

pub fn nested_model() -> NestedModel {
	NestedModel
}

pub fn local_facade_model(_model: &local_facade::SharedModel) {}
