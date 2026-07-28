use public_api::api::{AliasedModel, DeepModel, SharedModel};
use public_api::facade::ExternalModel;
use public_api::model_facade::NestedModel;

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
