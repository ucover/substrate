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

use crate::{BalanceOf, Config};
use frame_support::{dispatch::{DispatchError, DispatchResult}, DefaultNoBound};
use sp_core::crypto::UncheckedFrom;
use sp_runtime::traits::Saturating;
use sp_std::marker::PhantomData;

pub trait Ext<T: Config> {
	fn reserve_limit(origin: &T::AccountId, limit: &BalanceOf<T>) -> DispatchResult;
	fn charge(origin: &T::AccountId, contract: &T::AccountId) -> DispatchResult;
}

pub struct Meter<'parent, T: Config, E> {
	parent: Parent<'parent, T, E>,
	contract: T::AccountId,
	limit: BalanceOf<T>,
	total_usage: Usage<T>,
	own_usage: Usage<T>,
	_handler: PhantomData<E>,
}

pub enum Cost<T: Config> {
	Charge(BalanceOf<T>),
	Refund(BalanceOf<T>),
}

enum Parent<'parent, T: Config, E> {
	Origin(T::AccountId),
	Meter(&'parent mut Meter<'parent, T, E>),
}

impl<'parent, T: Config, E> Parent<'parent, T, E> {
	fn as_meter(&'parent mut self) -> Option<&mut Meter<'parent, T, E>> {
		match self {
			Self::Meter(meter) => Some(meter),
			Self::Origin(_) => None,
		}
	}

	fn origin(&'parent self) -> &T::AccountId {
		let mut current = self;
		loop {
			match current {
				Self::Meter(meter) => current = &meter.parent,
				Self::Origin(origin) => return &origin,
			}
		}
	}
}

#[derive(DefaultNoBound, Clone)]
struct Usage<T: Config> {
	charged: BalanceOf<T>,
	refunded: BalanceOf<T>,
}

impl<'parent, T, E> Meter<'parent, T, E>
where
	T: Config,
	T::AccountId: UncheckedFrom<T::Hash> + AsRef<[u8]>,
	E: Ext<T>,
{
	pub fn new(
		origin: T::AccountId,
		contract: T::AccountId,
		limit: BalanceOf<T>,
	) -> Result<Self, DispatchError> {
		E::reserve_limit(&origin, &limit)?;
		Ok(Self {
			parent: Parent::Origin(origin),
			contract,
			limit,
			total_usage: Default::default(),
			own_usage: Default::default(),
			_handler: PhantomData,
		})
	}

	pub fn nested(&'parent mut self, contract: T::AccountId) -> Self {
		let limit = self.available();
		let total_usage = self.total_usage.clone();
		Self {
			parent: Parent::Meter(self),
			contract,
			limit,
			total_usage,
			own_usage: Default::default(),
			_handler: PhantomData,
		}
	}

	pub fn absorb_nested(&'parent mut self) -> DispatchResult {
		let parent = self.parent.as_meter().expect("Due to typestate this always a nested meter; qed");
		unimplemented!()
	}

	fn available(&self) -> BalanceOf<T> {
		self.limit.saturating_add(self.total_usage.refunded).saturating_sub(self.total_usage.charged)
	}
}
