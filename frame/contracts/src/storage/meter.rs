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

use crate::{Config, BalanceOf};
use frame_support::dispatch::{DispatchResult, DispatchError};
use sp_core::crypto::UncheckedFrom;
use sp_std::marker::PhantomData;
use sp_runtime::traits::{Zero, Saturating};

trait Handler<T: Config> {
	fn reserve_limit(origin: &T::AccountId, limit: &BalanceOf<T>) -> DispatchResult;
}

struct Meter<T: Config, H> {
	origin: T::AccountId,
	contract: T::AccountId,
	limit: BalanceOf<T>,
	used: BalanceOf<T>,
	refunded: BalanceOf<T>,
	_handler: PhantomData<H>,
}

pub enum Charge {
	Charge(u32),
	Refund(u32),
}

impl<T, H> Meter<T, H>
where
	T: Config,
	T::AccountId: UncheckedFrom<T::Hash> + AsRef<[u8]>,
	H: Handler<T>,
{
	pub fn new(origin: T::AccountId, contract: T::AccountId, limit: BalanceOf<T>) -> Result<Self, DispatchError> {
		H::reserve_limit(&origin, &limit)?;
		Ok(Self {
			origin,
			contract,
			limit,
			used: <BalanceOf<T>>::zero(),
			refunded: <BalanceOf<T>>::zero(),
			_handler: PhantomData,
		})
	}

	pub fn nested(&mut self, contract: T::AccountId) -> Self {
		Self {
			origin: self.origin.clone(),
			contract,
			limit: self.available(),
			used: <BalanceOf<T>>::zero(),
			refunded: <BalanceOf<T>>::zero(),
			_handler: PhantomData,
		}
	}

	fn available(&self) -> BalanceOf<T> {
		self.limit.saturating_add(self.refunded).saturating_sub(self.used)
	}
}