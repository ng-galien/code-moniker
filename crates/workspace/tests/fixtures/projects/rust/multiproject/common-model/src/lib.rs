pub mod risk {
	use crate::CustomerId;

	pub struct RiskPolicy;

	pub fn assess(customer: &CustomerId) -> RiskPolicy {
		let _customer = customer;
		RiskPolicy
	}
}

pub struct CustomerId(pub String);

impl CustomerId {
	pub fn tag(_: &CustomerId) -> &'static str {
		"customer"
	}
}

pub enum Region {
	Eu,
}

macro_rules! define_languages {
	($($variant:ident => $module:ty),* $(,)?) => {
		pub enum Lang {
			$($variant,)*
		}
	}
}

define_languages! {
	Ts => crate::risk::RiskPolicy,
	Rs => crate::CustomerId,
}

pub const DEFAULT_REGION: &str = "eu";

pub struct OrderTotal(pub u64);

pub trait Auditable {}

pub fn normalize_customer(customer: CustomerId) -> CustomerId {
	customer
}
