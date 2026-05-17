use crate::ui::contracts::Effect;

#[derive(Debug)]
pub(super) struct Transition {
	pub(super) changed: bool,
	#[allow(dead_code)]
	pub(super) reason: &'static str,
	pub(super) effects: Vec<Effect>,
}

impl Transition {
	pub(super) const fn changed(reason: &'static str) -> Self {
		Self {
			changed: true,
			reason,
			effects: Vec::new(),
		}
	}

	pub(super) const fn unchanged(reason: &'static str) -> Self {
		Self {
			changed: false,
			reason,
			effects: Vec::new(),
		}
	}

	#[allow(dead_code)]
	pub(super) fn with_effect(mut self, effect: Effect) -> Self {
		self.effects.push(effect);
		self
	}

	pub(super) fn take_effects(&mut self) -> Vec<Effect> {
		std::mem::take(&mut self.effects)
	}
}

pub(super) trait Reduce<Action> {
	fn reduce(&mut self, action: Action) -> Transition;
}

#[derive(Debug)]
pub(super) struct ReactiveStore<State> {
	state: State,
	version: u64,
	last_transition: Transition,
}

impl<State> ReactiveStore<State> {
	pub(super) fn new(state: State) -> Self {
		Self {
			state,
			version: 0,
			last_transition: Transition::changed("init"),
		}
	}

	pub(super) fn state(&self) -> &State {
		&self.state
	}

	pub(super) fn select<'a, T>(&'a self, selector: impl FnOnce(&'a State) -> T) -> T {
		selector(&self.state)
	}
}

impl<State> ReactiveStore<State> {
	pub(super) fn dispatch<Action>(&mut self, action: Action) -> &mut Transition
	where
		State: Reduce<Action>,
	{
		self.last_transition = self.state.reduce(action);
		if self.last_transition.changed {
			self.version += 1;
		}
		&mut self.last_transition
	}

	pub(super) fn reduce_with(
		&mut self,
		reduce: impl FnOnce(&mut State) -> Transition,
	) -> &mut Transition {
		self.last_transition = reduce(&mut self.state);
		if self.last_transition.changed {
			self.version += 1;
		}
		&mut self.last_transition
	}
}
