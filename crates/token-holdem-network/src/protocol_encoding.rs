use token_holdem_domain::{StakeLevel, TableId};

pub(crate) fn write_bytes(output: &mut Vec<u8>, value: &[u8]) {
    output.extend_from_slice(&(value.len() as u32).to_be_bytes());
    output.extend_from_slice(value);
}

pub(crate) fn write_addresses(output: &mut Vec<u8>, addresses: &[Vec<u8>]) {
    output.extend_from_slice(&(addresses.len() as u32).to_be_bytes());
    for address in addresses {
        write_bytes(output, address);
    }
}

pub(crate) fn write_level(output: &mut Vec<u8>, level: &StakeLevel) {
    write_bytes(output, level.id().as_bytes());
    output.extend_from_slice(&level.small_blind().value().to_be_bytes());
    output.extend_from_slice(&level.big_blind().value().to_be_bytes());
    output.extend_from_slice(&level.minimum_buy_in().value().to_be_bytes());
    output.extend_from_slice(&level.maximum_buy_in().value().to_be_bytes());
    output.push(level.minimum_players());
    output.push(level.maximum_players());
}

pub(crate) fn write_table_id(output: &mut Vec<u8>, table_id: TableId) {
    output.extend_from_slice(table_id.as_bytes());
}

pub(crate) fn write_optional_hash(output: &mut Vec<u8>, hash: Option<[u8; 32]>) {
    match hash {
        Some(hash) => {
            output.push(1);
            output.extend_from_slice(&hash);
        }
        None => output.push(0),
    }
}
