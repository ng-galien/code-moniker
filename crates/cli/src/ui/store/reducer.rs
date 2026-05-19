use crate::ui::app::Effect;

#[derive(Debug)]
pub(in crate::ui) struct Transition {
	pub(in crate::ui) changed: bool,
	pub(in crate::ui) effects: Vec<Effect>,
}

pub(in crate::ui) struct Reduction<Outcome> {
	pub(in crate::ui) transition: Transition,
	pub(in crate::ui) outcome: Outcome,
}

impl Transition {
	pub(in crate::ui) const fn changed() -> Self {
		Self {
			changed: true,
			effects: Vec::new(),
		}
	}

	pub(in crate::ui) const fn unchanged() -> Self {
		Self {
			changed: false,
			effects: Vec::new(),
		}
	}

	pub(in crate::ui) fn with_effect(mut self, effect: Effect) -> Self {
		self.effects.push(effect);
		self
	}

	pub(in crate::ui) fn with_outcome<Outcome>(self, outcome: Outcome) -> Reduction<Outcome> {
		Reduction {
			transition: self,
			outcome,
		}
	}

	pub(in crate::ui) fn take_effects(&mut self) -> Vec<Effect> {
		std::mem::take(&mut self.effects)
	}
}

pub(in crate::ui) trait Reduce<Action> {
	fn reduce(&mut self, action: Action) -> Transition;
}

#[derive(Debug)]
pub(in crate::ui) struct ReducerStore<State> {
	state: State,
	last_transition: Transition,
}

impl<State> ReducerStore<State> {
	pub(in crate::ui) fn new(state: State) -> Self {
		Self {
			state,
			last_transition: Transition::changed(),
		}
	}

	pub(in crate::ui) fn state(&self) -> &State {
		&self.state
	}

	pub(in crate::ui) fn select<'a, T>(&'a self, selector: impl FnOnce(&'a State) -> T) -> T {
		selector(&self.state)
	}
}

impl<State> ReducerStore<State> {
	pub(in crate::ui) fn dispatch<Action>(&mut self, action: Action) -> &mut Transition
	where
		State: Reduce<Action>,
	{
		self.last_transition = self.state.reduce(action);
		&mut self.last_transition
	}

	pub(in crate::ui) fn reduce_with(
		&mut self,
		reduce: impl FnOnce(&mut State) -> Transition,
	) -> &mut Transition {
		self.last_transition = reduce(&mut self.state);
		&mut self.last_transition
	}

	pub(in crate::ui) fn reduce_with_outcome<Outcome>(
		&mut self,
		reduce: impl FnOnce(&mut State) -> Reduction<Outcome>,
	) -> (&mut Transition, Outcome) {
		let reduction = reduce(&mut self.state);
		let outcome = reduction.outcome;
		self.last_transition = reduction.transition;
		(&mut self.last_transition, outcome)
	}
}
