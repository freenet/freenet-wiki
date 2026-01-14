//! Wiki contract implementing Freenet ContractInterface.

use ciborium::{from_reader, into_writer};
use freenet_stdlib::prelude::*;
use freenet_wiki_common::{WikiParameters, WikiStateDelta, WikiStateSummary, WikiStateV1};

/// Local contract struct to implement ContractInterface.
#[allow(dead_code)]
struct Contract;

#[contract]
impl ContractInterface for Contract {
    fn validate_state(
        parameters: Parameters<'static>,
        state: State<'static>,
        _related: RelatedContracts<'static>,
    ) -> Result<ValidateResult, ContractError> {
        let bytes = state.as_ref();
        if bytes.is_empty() {
            return Ok(ValidateResult::Valid);
        }

        let params: WikiParameters =
            from_reader(parameters.as_ref()).map_err(|e| ContractError::Deser(e.to_string()))?;
        let wiki_state: WikiStateV1 =
            from_reader(bytes).map_err(|e| ContractError::Deser(e.to_string()))?;

        // Verify the entire state
        wiki_state
            .verify(&params)
            .map_err(|_| ContractError::InvalidState)?;

        Ok(ValidateResult::Valid)
    }

    fn update_state(
        parameters: Parameters<'static>,
        state: State<'static>,
        data: Vec<UpdateData<'static>>,
    ) -> Result<UpdateModification<'static>, ContractError> {
        let params: WikiParameters =
            from_reader(parameters.as_ref()).map_err(|e| ContractError::Deser(e.to_string()))?;
        let mut wiki_state: WikiStateV1 =
            from_reader(state.as_ref()).map_err(|e| ContractError::Deser(e.to_string()))?;

        for update in data {
            match update {
                UpdateData::State(new_state) => {
                    // Full state replacement - verify and merge
                    let incoming: WikiStateV1 = from_reader(new_state.as_ref())
                        .map_err(|e| ContractError::Deser(e.to_string()))?;
                    incoming
                        .verify(&params)
                        .map_err(|_| ContractError::InvalidState)?;

                    // Merge by computing delta from our summary and applying
                    let our_summary = wiki_state.summarize();
                    if let Some(delta) = incoming.delta(&our_summary) {
                        wiki_state
                            .apply_delta(&delta, &params)
                            .map_err(|_| ContractError::InvalidUpdate)?;
                    }
                }
                UpdateData::Delta(delta_state) => {
                    // Delta update - apply directly
                    let delta: WikiStateDelta = from_reader(delta_state.as_ref())
                        .map_err(|e| ContractError::Deser(e.to_string()))?;
                    wiki_state
                        .apply_delta(&delta, &params)
                        .map_err(|_| ContractError::InvalidUpdate)?;
                }
                _ => {}
            }
        }

        // Verify final state
        wiki_state
            .verify(&params)
            .map_err(|_| ContractError::InvalidState)?;

        let mut updated_state = Vec::new();
        into_writer(&wiki_state, &mut updated_state)
            .map_err(|e| ContractError::Deser(e.to_string()))?;

        Ok(UpdateModification::valid(updated_state.into()))
    }

    fn summarize_state(
        _parameters: Parameters<'static>,
        state: State<'static>,
    ) -> Result<StateSummary<'static>, ContractError> {
        let bytes = state.as_ref();
        if bytes.is_empty() {
            return Ok(StateSummary::from(Vec::new()));
        }

        let wiki_state: WikiStateV1 =
            from_reader(bytes).map_err(|e| ContractError::Deser(e.to_string()))?;

        let summary = wiki_state.summarize();
        let mut summary_bytes = Vec::new();
        into_writer(&summary, &mut summary_bytes)
            .map_err(|e| ContractError::Deser(e.to_string()))?;

        Ok(StateSummary::from(summary_bytes))
    }

    fn get_state_delta(
        _parameters: Parameters<'static>,
        state: State<'static>,
        summary: StateSummary<'static>,
    ) -> Result<StateDelta<'static>, ContractError> {
        let bytes = state.as_ref();
        if bytes.is_empty() {
            return Ok(StateDelta::from(Vec::new()));
        }

        let wiki_state: WikiStateV1 =
            from_reader(bytes).map_err(|e| ContractError::Deser(e.to_string()))?;

        let summary_bytes = summary.as_ref();
        if summary_bytes.is_empty() {
            // No summary means they have nothing, send full state as delta
            let mut delta_bytes = Vec::new();
            into_writer(&wiki_state, &mut delta_bytes)
                .map_err(|e| ContractError::Deser(e.to_string()))?;
            return Ok(StateDelta::from(delta_bytes));
        }

        let old_summary: WikiStateSummary =
            from_reader(summary_bytes).map_err(|e| ContractError::Deser(e.to_string()))?;

        match wiki_state.delta(&old_summary) {
            Some(delta) => {
                let mut delta_bytes = Vec::new();
                into_writer(&delta, &mut delta_bytes)
                    .map_err(|e| ContractError::Deser(e.to_string()))?;
                Ok(StateDelta::from(delta_bytes))
            }
            None => Ok(StateDelta::from(Vec::new())),
        }
    }
}
