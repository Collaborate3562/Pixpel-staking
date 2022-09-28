#![cfg_attr(not(feature = "std"), no_std)]

use concordium_cis2::*;
use concordium_std::*;

type ContractTokenId = TokenIdU32;

type ContractTokenAmount = TokenAmountU8;

#[derive(Serial, DeserialWithState, Deletable, StateClone)]
#[concordium(state_parameter = "S")]
struct StakeState<S> {
    staked_tokens: StateSet<ContractTokenId, S>,
    staked_token_price: u64,
    staked_start_at: StateMap<ContractTokenId, u64, S>
}

impl<S: HasStateApi> StakeState<S> {
    fn empty(state_builder: &mut StateBuilder<S>) -> Self {
        StakeState {
            staked_tokens: state_builder.new_set(),
            staked_token_price: 0,
            staked_start_at: state_builder.new_map()
        }
    }
}

#[derive(Serial, DeserialWithState, StateClone)]
#[concordium(state_parameter = "S")]
struct State<S> {
    stake:        StateMap<AccountAddress, StakeState<S>, S>,
    all_tokens:   StateSet<ContractTokenId, S>,
    total_staked: u64
}

#[derive(Serialize, Debug, PartialEq, Eq, Reject, SchemaType)]
enum CustomContractError {
    #[from(ParseError)]
    ParseParams,
    TokenNotFound,
    TokenAlreadyStaked,
    InvokeContractError,
}

type ContractError = Cis2Error<CustomContractError>;

type ContractResult<A> = Result<A, ContractError>;

/// Mapping errors related to contract invocations to CustomContractError.
impl<T> From<CallContractError<T>> for CustomContractError {
    fn from(_cce: CallContractError<T>) -> Self { Self::InvokeContractError }
}

impl From<CustomContractError> for ContractError {
    fn from(c: CustomContractError) -> Self { Cis2Error::Custom(c) }
}

impl<S: HasStateApi> State<S> {
    /// Creates a new state with no tokens.
    fn empty(state_builder: &mut StateBuilder<S>) -> Self {
        State {
            stake:        state_builder.new_map(),
            all_tokens:   state_builder.new_set(),
            total_staked: 0,
        }
    }

    fn insert_token(
        &mut self,
        token: ContractTokenId,
        owner: &AccountAddress,
        staked_time: u64,
        state_builder: &mut StateBuilder<S>,
    ) -> ContractResult<()> {
        ensure!(self.all_tokens.insert(token), CustomContractError::TokenAlreadyStaked.into());

        let mut owner_state =
            self.stake.entry(*owner).or_insert_with(|| StakeState::empty(state_builder));
        owner_state.staked_tokens.insert(token);
        owner_state.staked_start_at.insert(token, staked_time);
        Ok(())
    }

    fn remove_token(
        &mut self,
        token: ContractTokenId,
        owner: &AccountAddress,
        staked_time: u64,
        state_builder: &mut StateBuilder<S>,
    ) -> ContractResult<()> {
        ensure!(self.all_tokens.remove(&token), CustomContractError::TokenAlreadyStaked.into());

        let mut owner_state =
            self.stake.entry(*owner).or_insert_with(|| StakeState::empty(state_builder));
        owner_state.staked_tokens.remove(&token);
        owner_state.staked_start_at.remove(&token);
        Ok(())
    }
    fn increase_price(
        &mut self,
        owner: &AccountAddress,
        staked_price: u64,
        state_builder: &mut StateBuilder<S>,
    ) -> ContractResult<()> {
        self.total_staked += staked_price;
        let mut owner_state =
            self.stake.entry(*owner).or_insert_with(|| StakeState::empty(state_builder));
        owner_state.staked_token_price += staked_price;
        Ok(())
    }

    fn decrease_price(
        &mut self,
        owner: &AccountAddress,
        staked_price: u64,
        state_builder: &mut StateBuilder<S>,
    ) -> ContractResult<()> {
        self.total_staked -= staked_price;
        let mut owner_state =
            self.stake.entry(*owner).or_insert_with(|| StakeState::empty(state_builder));
        owner_state.staked_token_price -= staked_price;
        Ok(())
    }
}

#[init(contract = "nft-staking")]
fn contract_init<S: HasStateApi>(
    _ctx: &impl HasInitContext,
    state_builder: &mut StateBuilder<S>,
) -> InitResult<State<S>> {
    // Construct the initial contract state.
    Ok(State::empty(state_builder))
}

#[derive(Serial, Deserial, SchemaType)]
struct StakeParams {
    owner:  AccountAddress,
    #[concordium(size_length = 1)]
    tokens: collections::BTreeSet<ContractTokenId>,
    price: Amount,
}

#[receive(
    contract = "nft-staking",
    name = "stake",
    parameter = "StakeParams",
    error = "ContractError",
    mutable
)]
fn stake_nft<S: HasStateApi>(
    ctx: &impl HasReceiveContext,
    host: &mut impl HasHost<State<S>, StateApiType = S>,
) -> ContractResult<()> {
    let params: StakeParams = ctx.parameter_cursor().get()?;
    let sender = ctx.sender();

    ensure!(sender.matches_account(&params.owner), ContractError::Unauthorized);

    let (state, builder) = host.state_and_builder();

    for &token_id in params.tokens.iter() {
        let slot_time = ctx.metadata().slot_time();
        state.insert_token(token_id, &params.owner, Timestamp::timestamp_millis(&slot_time), builder)?;
    }

    let price = params.price;

    state.increase_price(&params.owner, price.micro_ccd, builder);

    Ok(())
}

#[derive(Serial, Deserial, SchemaType)]
struct UnStakeParams {
    owner:  AccountAddress,
    #[concordium(size_length = 1)]
    tokens: collections::BTreeSet<ContractTokenId>,
    price: Amount,
}

#[receive(
    contract = "nft-staking",
    name = "unstake",
    parameter = "UnStakeParams",
    error = "ContractError",
    mutable
)]
fn unstake_nft<S: HasStateApi>(
    ctx: &impl HasReceiveContext,
    host: &mut impl HasHost<State<S>, StateApiType = S>,
) -> ContractResult<()> {
    let params: UnStakeParams = ctx.parameter_cursor().get()?;
    let sender = ctx.sender();

    ensure!(sender.matches_account(&params.owner), ContractError::Unauthorized);

    let (state, builder) = host.state_and_builder();

    for &token_id in params.tokens.iter() {
        let slot_time = ctx.metadata().slot_time();
        state.remove_token(token_id, &params.owner, Timestamp::timestamp_millis(&slot_time), builder)?;
    }

    let price = params.price;

    state.decrease_price(&params.owner, price.micro_ccd, builder);

    Ok(())
}

#[derive(Serial, Deserial, SchemaType)]
struct ClaimParams {
    owner:  AccountAddress
}


#[receive(
    contract = "nft-staking",
    name = "claim",
    parameter = "ClaimParams",
    error = "ContractError",
    mutable
)]
fn claim_reward<S: HasStateApi>(
    ctx: &impl HasReceiveContext,
    host: &mut impl HasHost<State<S>, StateApiType = S>,
) -> ContractResult<()> {
    let params: ClaimParams = ctx.parameter_cursor().get()?;
    let sender = ctx.sender();

    ensure!(sender.matches_account(&params.owner), ContractError::Unauthorized);

    let (state, builder) = host.state_and_builder();

    // staking strategy
    Ok(())
}

