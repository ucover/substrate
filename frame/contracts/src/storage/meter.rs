// This file is part of Substrate.

// Copyright (C) 2019-2021 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: Apache-2.0

// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
// 	http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! This module contains functions to meter the storage usage.

use crate::{BalanceOf, Config, Error};
use frame_support::{
	dispatch::{DispatchError, DispatchResult},
	DefaultNoBound,
};
use sp_core::crypto::UncheckedFrom;
use sp_runtime::traits::{Saturating, Zero};
use sp_std::marker::PhantomData;

pub type Meter<T> = RawMeter<T, DefaultExt, state::Root>;
pub type NestedMeter<T> = RawMeter<T, DefaultExt, state::Nested>;

pub trait Ext<T: Config> {
	fn reserve_limit(origin: &T::AccountId, limit: &BalanceOf<T>) -> DispatchResult;
	fn unreserve_limit(origin: &T::AccountId, limit: &BalanceOf<T>, usage: &Usage<T>);
	fn charge(origin: &T::AccountId, contract: &T::AccountId, amount: &Cost<T>);
}

pub struct RawMeter<T: Config, E: Ext<T>, S: state::State> {
	origin: Option<T::AccountId>,
	limit: BalanceOf<T>,
	total_usage: Usage<T>,
	own_usage: Usage<T>,
	_phantom: PhantomData<(E, S)>,
}

pub enum Cost<T: Config> {
	Charge(BalanceOf<T>),
	Refund(BalanceOf<T>),
}

#[derive(DefaultNoBound, Clone)]
pub struct Usage<T: Config> {
	charge: BalanceOf<T>,
	refund: BalanceOf<T>,
}

pub enum DefaultExt {}

impl<T: Config> Copy for Usage<T> {}

impl<T: Config> Usage<T> {
	fn cost(&self) -> Cost<T> {
		if self.charge >= self.refund {
			Cost::Charge(self.charge.saturating_sub(self.refund))
		} else {
			Cost::Refund(self.refund.saturating_sub(self.charge))
		}
	}
}

impl<T: Config> Saturating for Usage<T> {
	fn saturating_add(self, rhs: Self) -> Self {
		Self {
			charge: self.charge.saturating_add(rhs.charge),
			refund: self.refund.saturating_add(rhs.refund),
		}
	}

	fn saturating_sub(self, rhs: Self) -> Self {
		Self {
			charge: self.charge.saturating_sub(rhs.charge),
			refund: self.refund.saturating_sub(rhs.refund),
		}
	}

	fn saturating_mul(self, rhs: Self) -> Self {
		Self {
			charge: self.charge.saturating_mul(rhs.charge),
			refund: self.refund.saturating_mul(rhs.refund),
		}
	}

	fn saturating_pow(self, exp: usize) -> Self {
		Self {
			charge: self.charge.saturating_pow(exp),
			refund: self.refund.saturating_pow(exp),
		}
	}
}

impl<T, E, S> Drop for RawMeter<T, E, S>
where
	T: Config,
	E: Ext<T>,
	S: state::State,
{
	fn drop(&mut self) {
		// Drop cannot be specialized: We need to do a runtime check.
		if let Some(origin) = self.origin.as_ref() {
			// you cannot charge to the root meter
			debug_assert_eq!(self.own_usage.charge, <BalanceOf<T>>::zero());
			debug_assert_eq!(self.own_usage.refund, <BalanceOf<T>>::zero());
			E::unreserve_limit(origin, &self.limit, &self.total_usage);
		}
	}
}

impl<T, E, S> RawMeter<T, E, S>
where
	T: Config,
	T::AccountId: UncheckedFrom<T::Hash> + AsRef<[u8]>,
	E: Ext<T>,
	S: state::State,
{
	pub fn nested(&mut self, contract: T::AccountId) -> RawMeter<T, E, state::Nested> {
		RawMeter {
			origin: None,
			limit: self.available(),
			total_usage: Default::default(),
			own_usage: Default::default(),
			_phantom: PhantomData,
		}
	}

	pub fn absorb(
		&mut self,
		absorbed: &mut RawMeter<T, E, state::Nested>,
		origin: &T::AccountId,
		contract: &T::AccountId,
	) {
		E::charge(origin, &contract, &absorbed.own_usage.cost());
		self.total_usage = self.total_usage.saturating_add(absorbed.total_usage);
		absorbed.limit = Default::default();
		absorbed.total_usage = Default::default();
		absorbed.own_usage = Default::default();
	}

	fn available(&self) -> BalanceOf<T> {
		self.limit
			.saturating_add(self.total_usage.refund)
			.saturating_sub(self.total_usage.charge)
	}
}

impl<T, E> RawMeter<T, E, state::Root>
where
	T: Config,
	T::AccountId: UncheckedFrom<T::Hash> + AsRef<[u8]>,
	E: Ext<T>,
{
	pub fn new(origin: T::AccountId, limit: BalanceOf<T>) -> Result<Self, DispatchError> {
		E::reserve_limit(&origin, &limit)?;
		Ok(Self {
			origin: Some(origin),
			limit,
			total_usage: Default::default(),
			own_usage: Default::default(),
			_phantom: PhantomData,
		})
	}
}

impl<T, E> RawMeter<T, E, state::Nested>
where
	T: Config,
	T::AccountId: UncheckedFrom<T::Hash> + AsRef<[u8]>,
	E: Ext<T>,
{
	pub fn charge(&mut self, usage: Usage<T>) -> DispatchResult {
		self.total_usage = self.total_usage.saturating_add(usage);
		self.own_usage = self.own_usage.saturating_add(usage);
		if let Cost::Charge(amount) = self.total_usage.cost() {
			if amount > self.limit {
				return Err(<Error<T>>::StorageExhausted.into())
			}
		}
		Ok(())
	}
}

impl<T: Config> Ext<T> for DefaultExt {
	fn reserve_limit(origin: &T::AccountId, limit: &BalanceOf<T>) -> DispatchResult {
		unimplemented!()
	}

	fn unreserve_limit(origin: &T::AccountId, limit: &BalanceOf<T>, usage: &Usage<T>) {
		unimplemented!()
	}

	fn charge(origin: &T::AccountId, contract: &T::AccountId, amount: &Cost<T>) {
		unimplemented!()
	}
}

/// Private submodule with public types to prevent other modules from naming them.
mod state {
	pub trait State {}

	pub enum Root {}
	pub enum Nested {}

	impl State for Root {}
	impl State for Nested {}
}
