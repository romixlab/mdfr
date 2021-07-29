
use crate::mdfinfo::mdfinfo4::{Dl4Block, parser_dl4_block, parse_dz, Hl4Block, Dt4Block};
use crate::mdfinfo::mdfinfo4::{Cg4, Dg4, MdfInfo4, parse_block_header, Cc4Block, Cn4, CnType};
use std::{collections::HashMap, convert::TryInto, io::{BufReader, Read}, usize};
use std::fs::File;
use std::str;
use std::string::String;
use binread::BinReaderExt;
use byteorder::{BigEndian, LittleEndian, ReadBytesExt};
use num::Complex;
use half::f16;
use encoding_rs::{Decoder, UTF_16BE, UTF_16LE, WINDOWS_1252};
use ndarray::{Array1, OwnedRepr, ArrayBase, Dim, Zip};
use rayon::prelude::*;

// The following constant represents the of data chunk to be read and processed.
// a big chunk will improve performance but consume more memory
// a small wil not consume too much memory but will cause many read calls, penalising performance
const CHUNK_SIZE_READING: usize = 524288; // can be tuned according to architecture

/// Reads the file data based on headers information contained in info parameter
pub fn mdfreader4<'a>(rdr: &'a mut BufReader<&File>, info: &'a mut MdfInfo4) {
    let mut position: i64 = 0;
    // read file data
    for (_dg_position, dg) in info.dg.iter_mut() {
        if dg.block.dg_data != 0 {
            // header block
            rdr.seek_relative(dg.block.dg_data - position).expect("Could not position buffer");  // change buffer position
            let mut id = [0u8; 4];
            rdr.read_exact(&mut id).expect("could not read block id");
            position = read_data(rdr, id, dg, dg.block.dg_data);
        }
        apply_bit_mask_offset(dg);
        // channel_group invalid bits calculation
        for channel_group in dg.cg.values_mut() {
            // channel_group.process_all_channel_invalid_bits();
        }
        // conversion of all channels to physical values
        convert_all_channels(dg, &info.sharable.cc);
    }
}

/// Reads all kind of data layout : simple DT or DV, sorted or unsorted, Data List,
/// compressed data blocks DZ or Sample DATA
fn read_data(rdr: &mut BufReader<&File>, id: [u8; 4], dg: &mut Dg4, mut position: i64) -> i64 {
    // block header is already read
    let mut decoder: Dec = Dec {windows_1252: WINDOWS_1252.new_decoder(), utf_16_be: UTF_16BE.new_decoder(), utf_16_le: UTF_16LE.new_decoder()};
    if "##DT".as_bytes() == id {
        let block_header: Dt4Block = rdr.read_le().unwrap();
        // simple data block
        if dg.cg.len() == 1 {
            // sorted data group
            for channel_group in dg.cg.values_mut() {
                read_all_channels_sorted(rdr, channel_group);
            }
        } else if !dg.cg.is_empty() {
            // unsorted data
            // initialises all arrays
            for channel_group in dg.cg.values_mut() {
                initialise_arrays(channel_group, &channel_group.block.cg_cycle_count.clone());
            }
            read_all_channels_unsorted(rdr, dg, block_header.len as i64);
        }
        position += block_header.len as i64;
    } else if "##DZ".as_bytes() == id {
        let (mut data, block_header) = parse_dz(rdr);
        // compressed data
        if dg.cg.len() == 1 {
            // sorted data group
            for channel_group in dg.cg.values_mut() {
                read_all_channels_sorted_from_bytes(&data, channel_group);
            }
        } else if !dg.cg.is_empty() {
            // unsorted data
            // initialises all arrays
            for channel_group in dg.cg.values_mut() {
                initialise_arrays(channel_group, &channel_group.block.cg_cycle_count.clone());
            }
            // initialise record counter
            let mut record_counter: HashMap<u64, (usize, Vec<u8>)> = HashMap::new();
            for cg in dg.cg.values_mut() {
                record_counter.insert(cg.block.cg_record_id, (0, Vec::with_capacity((cg.record_length as u64 * cg.block.cg_cycle_count) as usize)));
            }
            read_all_channels_unsorted_from_bytes(&mut data, dg, &mut record_counter, &mut decoder);
        }
        position += block_header.len as i64;
    } else if "##HL".as_bytes() == id {
        // compressed data in datal list
        let block: Hl4Block = rdr.read_le().expect("could not read HL block");
        position += block.hl_len as i64;
        // Read Id of pointed DL Block
        rdr.seek_relative(block.hl_dl_first - position).expect("Could not reach DL block from HL block");
        position = block.hl_dl_first;
        let mut id = [0u8; 4];
        rdr.read_exact(&mut id).expect("could not read DL block id");
        // Read DL Blocks
        position = read_data(rdr, id, dg, position);
    } else if "##SD".as_bytes() == id {
        // signal data for VLSD
        let block_header: Dt4Block = rdr.read_le().unwrap();
        todo!();
    } else if "##DL".as_bytes() == id {
        // data list
        if dg.cg.len() == 1 {
            // sorted data group
            for channel_group in dg.cg.values_mut() {
                let (dl_blocks, pos) = parser_dl4(rdr, position);
                let pos = parser_dl4_sorted(rdr, dl_blocks, pos, channel_group);
                position = pos;
            }
        } else if !dg.cg.is_empty() {
            // unsorted data
            // initialises all arrays
            for channel_group in dg.cg.values_mut() {
                initialise_arrays(channel_group, &channel_group.block.cg_cycle_count.clone());
            }
            let (dl_blocks, pos) = parser_dl4(rdr, position);
            let pos = parser_dl4_unsorted(rdr, dg, dl_blocks, pos);
            position = pos;
        }
    } else if "##LD".as_bytes() == id {
        // list data
        todo!();
    }else if "##DV".as_bytes() == id {
        // data values
        // sorted data group only, no record id
        let block_header: Dt4Block = rdr.read_le().unwrap();
        for channel_group in dg.cg.values_mut() {
            read_all_channels_sorted(rdr, channel_group);
        }
        position += block_header.len as i64;
    }
    position
}

/// Reads all DL Blocks and returns a vect of them
fn parser_dl4(rdr: &mut BufReader<&File>, mut position: i64) -> (Vec<Dl4Block>, i64) {  
    let mut dl_blocks: Vec<Dl4Block> = Vec::new();
    let (block, pos) = parser_dl4_block(rdr, position, position);
    position = pos;
    dl_blocks.push(block.clone());
    let mut next_dl = block.dl_dl_next;
    while next_dl > 0 {
        let mut id = [0u8; 4];
        rdr.read_exact(&mut id).expect("could not read DL block id");
        position += 4;
        let (block, pos) = parser_dl4_block(rdr, block.dl_dl_next + 4, position);
        position = pos;
        dl_blocks.push(block.clone());
        next_dl = block.dl_dl_next;
    }
    (dl_blocks, position)
}

/// Reads all sorted data blocks pointed by DL4 Blocks
fn parser_dl4_sorted(rdr: &mut BufReader<&File>, dl_blocks: Vec<Dl4Block>, mut position: i64, channel_group: &mut Cg4) -> i64 {
    // initialises the arrays
    initialise_arrays(channel_group, &channel_group.block.cg_cycle_count.clone());
    // Read all data blocks
    let mut data: Vec<u8> = Vec::new();
    let mut previous_index: usize = 0;
    for dl in dl_blocks {
        for data_pointer in dl.dl_data {
            // Reads DT or DZ block id
            rdr.seek_relative(data_pointer - position).unwrap();
            let mut id = [0u8; 4];
            rdr.read_exact(&mut id).expect("could not read data block id");
            let block_length: usize;
            if id == "##DZ".as_bytes() {
                let (dt, block_header) = parse_dz(rdr);
                data.extend(dt);
                block_length = block_header.dz_org_data_length as usize;
                position = data_pointer + block_header.len as i64;
            } else {
                let block_header: Dt4Block = rdr.read_le().unwrap();
                let mut buf= vec![0u8; (block_header.len - 24) as usize];
                rdr.read_exact(&mut buf).unwrap();
                data.extend(buf);
                block_length = (block_header.len - 24) as usize;
                position = data_pointer + block_header.len as i64;
            }
            // Copies full sized records in block into channels arrays
            let record_length = channel_group.record_length as usize;
            let n_record_chunk = block_length / record_length;
            read_channels_from_bytes(&data[..record_length * n_record_chunk], &mut channel_group.cn, record_length, previous_index);
            // drop what has ben copied and keep remaining to be extended
            let remaining = block_length % record_length;
            if remaining > 0 {
                // copies tail part at beginnning of vect
                data.copy_within(record_length * n_record_chunk.., 0);
                // clears the last part
                data.truncate(remaining);
            } else {data.clear()}
            previous_index += n_record_chunk;
        }
    }
    position
}

/// Reads all unsorted data blocks pointed by DL4 Blocks
fn parser_dl4_unsorted(rdr: &mut BufReader<&File>, dg: &mut Dg4, dl_blocks: Vec<Dl4Block>, mut position: i64) -> i64 {
    // Read all data blocks
    let mut data: Vec<u8> = Vec::new();
    let mut decoder: Dec = Dec {windows_1252: WINDOWS_1252.new_decoder(), utf_16_be: UTF_16BE.new_decoder(), utf_16_le: UTF_16LE.new_decoder()};
    // initialise record counter
    let mut record_counter: HashMap<u64, (usize, Vec<u8>)> = HashMap::new();
    for cg in dg.cg.values_mut() {
        record_counter.insert(cg.block.cg_record_id, (0, Vec::new()));
    }
    for dl in dl_blocks {
        for data_pointer in dl.dl_data {
            rdr.seek_relative(data_pointer - position).unwrap();
            let header = parse_block_header(rdr);
            if header.hdr_id == "##DZ".as_bytes() {
                let (dt, _block) = parse_dz(rdr);
                data.extend(dt);
            } else {
                let mut buf= vec![0u8; (header.hdr_len - 24) as usize];
                rdr.read_exact(&mut buf).unwrap();
                data.extend(buf);
            }
            // saves records as much as possible
            read_all_channels_unsorted_from_bytes(&mut data, dg, &mut record_counter, &mut decoder);
            position = data_pointer + header.hdr_len as i64;
        }
    }
    position
}

/// Returns chunk size and corresponding number of records from a channel group
fn generate_chunks(channel_group: &Cg4) -> Vec<(usize, usize)>{
    let record_length = channel_group.record_length as usize;
    let cg_cycle_count = channel_group.block.cg_cycle_count as usize;
    let n_chunks = (record_length * cg_cycle_count) / CHUNK_SIZE_READING + 1; // number of chunks
    let chunk_length = (record_length * cg_cycle_count) / n_chunks; // chunks length
    let n_record_chunk = chunk_length / record_length; // number of records in chunk
    let chunck = (n_record_chunk, record_length * n_record_chunk);
    let mut chunks = vec![chunck; n_chunks];
    let n_record_chunk = cg_cycle_count - n_record_chunk * n_chunks;
    if n_record_chunk > 0 {
        chunks.push((n_record_chunk, record_length * n_record_chunk))
    }
    chunks
}

/// Reads all channels from given channel group having sorted data blocks
fn read_all_channels_sorted(rdr: &mut BufReader<&File>, channel_group: &mut Cg4) {
    let chunks =  generate_chunks(channel_group);
    // initialises the arrays
    initialise_arrays(channel_group, &channel_group.block.cg_cycle_count.clone());
    // read by chunks and store in channel array
    let mut previous_index: usize = 0;
    
    for (n_record_chunk, chunk_size) in chunks {
        let mut data_chunk= vec![0u8; chunk_size];
        rdr.read_exact(&mut data_chunk).expect("Could not read data chunk");
        read_channels_from_bytes(&data_chunk, &mut channel_group.cn, channel_group.record_length as usize, previous_index);
        previous_index += n_record_chunk;
    }
}

// copies data from data_chunk into each channel array
fn read_channels_from_bytes(data_chunk: &[u8], channels: &mut CnType, record_length: usize, previous_index: usize) {
    // iterates for each channel in parallel with rayon crate
    channels.par_iter_mut().for_each( |(_rec_pos, cn)| {
        if cn.block.cn_type == 0 || cn.block.cn_type == 2 || cn.block.cn_type == 4 {
            // fixed length data channel, master channel of synchronisation channel
            let mut value: &[u8];  // value of channel at record
            let pos_byte_beg = cn.pos_byte_beg as usize;
            match &mut cn.data {
                ChannelData::Int8(data) => {
                        for (i, record) in data_chunk.chunks(record_length).enumerate() {
                            value = &record[pos_byte_beg..pos_byte_beg + std::mem::size_of::<i8>()];
                            data[i + previous_index] = i8::from_le_bytes(value.try_into().expect("Could not read i8"));
                        }
                    },
                ChannelData::UInt8(data) => {
                        for (i, record) in data_chunk.chunks(record_length).enumerate() {
                            value = &record[pos_byte_beg..pos_byte_beg + std::mem::size_of::<u8>()];
                            data[i + previous_index] = u8::from_le_bytes(value.try_into().expect("Could not read u8"));
                        }
                    },
                ChannelData::Int16(data) => {
                        if cn.endian {
                            for (i, record) in data_chunk.chunks(record_length).enumerate() {
                                value = &record[pos_byte_beg..pos_byte_beg + std::mem::size_of::<i16>()];
                                data[i + previous_index] = i16::from_be_bytes(value.try_into().expect("Could not read be i16"));
                            }
                        } else {
                            for (i, record) in data_chunk.chunks(record_length).enumerate() {
                                value = &record[pos_byte_beg..pos_byte_beg + std::mem::size_of::<i16>()];
                                data[i + previous_index] = i16::from_le_bytes(value.try_into().expect("Could not read le i16"));
                            }
                        }
                    },
                ChannelData::UInt16(data) => {
                        if cn.endian {
                            for (i, record) in data_chunk.chunks(record_length).enumerate() {
                                value = &record[pos_byte_beg..pos_byte_beg + std::mem::size_of::<u16>()];
                                data[i + previous_index] = u16::from_be_bytes(value.try_into().expect("Could not read be u16"));
                            }
                        } else {
                            for (i, record) in data_chunk.chunks(record_length).enumerate() {
                                value = &record[pos_byte_beg..pos_byte_beg + std::mem::size_of::<u16>()];
                                data[i + previous_index] = u16::from_le_bytes(value.try_into().expect("Could not read le u16"));
                            }
                        }
                    },
                ChannelData::Float16(data) => {
                        if cn.endian {
                            for (i, record) in data_chunk.chunks(record_length).enumerate() {
                                value = &record[pos_byte_beg..pos_byte_beg + std::mem::size_of::<f16>()];
                                data[i + previous_index] = f16::from_be_bytes(value.try_into().expect("Could not read be f16")).to_f32();
                            }
                        } else {
                            for (i, record) in data_chunk.chunks(record_length).enumerate() {
                                value = &record[pos_byte_beg..pos_byte_beg + std::mem::size_of::<f16>()];
                                data[i + previous_index] = f16::from_le_bytes(value.try_into().expect("Could not read le f16")).to_f32();
                            }
                        }
                    },
                ChannelData::Int24(data) => {
                        if cn.endian {
                            for (i, record) in data_chunk.chunks(record_length).enumerate() {
                                value = &record[pos_byte_beg..pos_byte_beg + 3];
                                data[i + previous_index] = value.read_i24::<BigEndian>().expect("Could not read be i24");
                            }
                        } else {
                            for (i, record) in data_chunk.chunks(record_length).enumerate() {
                                value = &record[pos_byte_beg..pos_byte_beg + 3];
                                data[i + previous_index] = value.read_i24::<LittleEndian>().expect("Could not read le i24");
                            }
                        }
                    },
                ChannelData::UInt24(data) => {
                        if cn.endian {
                            for (i, record) in data_chunk.chunks(record_length).enumerate() {
                                value = &record[pos_byte_beg..pos_byte_beg + 3];
                                data[i + previous_index] = value.read_u24::<BigEndian>().expect("Could not read be u24");
                            }
                        } else {
                            for (i, record) in data_chunk.chunks(record_length).enumerate() {
                                value = &record[pos_byte_beg..pos_byte_beg + 3];
                                data[i + previous_index] = value.read_u24::<LittleEndian>().expect("Could not read le u24");
                            }
                        }
                    },
                ChannelData::Int32(data) => {
                        if cn.endian {
                            for (i, record) in data_chunk.chunks(record_length).enumerate() {
                                value = &record[pos_byte_beg..pos_byte_beg + std::mem::size_of::<i32>()];
                                data[i + previous_index] = i32::from_be_bytes(value.try_into().expect("Could not read be i32"));
                            }
                        } else {
                            for (i, record) in data_chunk.chunks(record_length).enumerate() {
                                value = &record[pos_byte_beg..pos_byte_beg + std::mem::size_of::<i32>()];
                                data[i + previous_index] = i32::from_le_bytes(value.try_into().expect("Could not read le i32"));
                            }
                        }
                    },
                ChannelData::UInt32(data) => {
                        if cn.endian {
                            for (i, record) in data_chunk.chunks(record_length).enumerate() {
                                value = &record[pos_byte_beg..pos_byte_beg + std::mem::size_of::<u32>()];
                                data[i + previous_index] = u32::from_be_bytes(value.try_into().expect("Could not read be u32"));
                            }
                        } else {
                            for (i, record) in data_chunk.chunks(record_length).enumerate() {
                                value = &record[pos_byte_beg..pos_byte_beg + std::mem::size_of::<u32>()];
                                data[i + previous_index] = u32::from_le_bytes(value.try_into().expect("Could not read le u32"));
                            }
                        }
                    },
                ChannelData::Float32(data) => {
                        if cn.endian {
                            for (i, record) in data_chunk.chunks(record_length).enumerate() {
                                value = &record[pos_byte_beg..pos_byte_beg + std::mem::size_of::<f32>()];
                                data[i + previous_index] = f32::from_be_bytes(value.try_into().expect("Could not read be u32"));
                            }
                        } else {
                            for (i, record) in data_chunk.chunks(record_length).enumerate() {
                                value = &record[pos_byte_beg..pos_byte_beg + std::mem::size_of::<f32>()];
                                data[i + previous_index] = f32::from_le_bytes(value.try_into().expect("Could not read le u32"));
                            }
                        }
                    },
                ChannelData::Int48(data) => {
                        if cn.endian {
                            for (i, record) in data_chunk.chunks(record_length).enumerate() {
                                value = &record[pos_byte_beg..pos_byte_beg + 6];
                                data[i + previous_index] = value.read_i48::<BigEndian>().expect("Could not read be i48");
                            }
                        } else {
                            for (i, record) in data_chunk.chunks(record_length).enumerate() {
                                value = &record[pos_byte_beg..pos_byte_beg + 6];
                                data[i + previous_index] = value.read_i48::<LittleEndian>().expect("Could not read le i48");
                            }
                        }
                    },
                ChannelData::UInt48(data) => {
                        if cn.endian {
                            for (i, record) in data_chunk.chunks(record_length).enumerate() {
                                value = &record[pos_byte_beg..pos_byte_beg + 6];
                                data[i + previous_index] = value.read_u48::<BigEndian>().expect("Could not read be u48");
                            }
                        } else {
                            for (i, record) in data_chunk.chunks(record_length).enumerate() {
                                value = &record[pos_byte_beg..pos_byte_beg + 6];
                                data[i + previous_index] = value.read_u48::<LittleEndian>().expect("Could not read le u48");
                            }
                        }
                    },
                ChannelData::Int64(data) => {
                        if cn.endian {
                            for (i, record) in data_chunk.chunks(record_length).enumerate() {
                                value = &record[pos_byte_beg..pos_byte_beg + std::mem::size_of::<i64>()];
                                data[i + previous_index] = i64::from_be_bytes(value.try_into().expect("Could not read be i64"));
                            }
                        } else {
                            for (i, record) in data_chunk.chunks(record_length).enumerate() {
                                value = &record[pos_byte_beg..pos_byte_beg + std::mem::size_of::<i64>()];
                                data[i + previous_index] = i64::from_le_bytes(value.try_into().expect("Could not read le i64"));
                            }
                        }
                    },
                ChannelData::UInt64(data) => {
                        if cn.endian {
                            for (i, record) in data_chunk.chunks(record_length).enumerate() {
                                value = &record[pos_byte_beg..pos_byte_beg + std::mem::size_of::<u64>()];
                                data[i + previous_index] = u64::from_be_bytes(value.try_into().expect("Could not read be u64"));
                            }
                        } else {
                            for (i, record) in data_chunk.chunks(record_length).enumerate() {
                                value = &record[pos_byte_beg..pos_byte_beg + std::mem::size_of::<u64>()];
                                data[i + previous_index] = u64::from_le_bytes(value.try_into().expect("Could not read le u64"));
                            }
                        }
                    },
                ChannelData::Float64(data) => {
                        if cn.endian {
                            for (i, record) in data_chunk.chunks(record_length).enumerate() {
                                value = &record[pos_byte_beg..pos_byte_beg + std::mem::size_of::<f64>()];
                                data[i + previous_index] = f64::from_be_bytes(value.try_into().expect("Could not read be f64"));
                            }
                        } else {
                            for (i, record) in data_chunk.chunks(record_length).enumerate() {
                                value = &record[pos_byte_beg..pos_byte_beg + std::mem::size_of::<f64>()];
                                data[i + previous_index] = f64::from_le_bytes(value.try_into().expect("Could not read le f64"));
                            }
                        }
                    },
                ChannelData::Complex16(data) => {
                    let mut re: f32;
                    let mut im: f32;
                    let mut re_val: &[u8];
                    let mut im_val: &[u8];
                    if cn.endian {
                        for (i, record) in data_chunk.chunks(record_length).enumerate() {
                            re_val = &record[pos_byte_beg..pos_byte_beg + std::mem::size_of::<f16>()];
                            im_val = &record[pos_byte_beg + std::mem::size_of::<f16>()..pos_byte_beg + 2 * std::mem::size_of::<f16>()];
                            re = f16::from_be_bytes(re_val.try_into().expect("Could not read be real f16 complex")).to_f32();
                            im = f16::from_be_bytes(im_val.try_into().expect("Could not read be img f16 complex")).to_f32();
                            data[i + previous_index] = Complex::new(re, im);
                        }
                    } else {
                        for (i, record) in data_chunk.chunks(record_length).enumerate() {
                            re_val = &record[pos_byte_beg..pos_byte_beg + std::mem::size_of::<f16>()];
                            im_val = &record[pos_byte_beg + std::mem::size_of::<f16>()..pos_byte_beg + 2 * std::mem::size_of::<f16>()];
                            re = f16::from_le_bytes(re_val.try_into().expect("Could not read le real f16 complex")).to_f32();
                            im = f16::from_le_bytes(im_val.try_into().expect("Could not read le img f16 complex")).to_f32();
                            data[i + previous_index] = Complex::new(re, im);}
                        }
                    },
                ChannelData::Complex32(data) => {
                    let mut re: f32;
                    let mut im: f32;
                    let mut re_val: &[u8];
                    let mut im_val: &[u8];
                    if cn.endian {
                        for (i, record) in data_chunk.chunks(record_length).enumerate() {
                            re_val = &record[pos_byte_beg..pos_byte_beg + std::mem::size_of::<f32>()];
                            im_val = &record[pos_byte_beg + std::mem::size_of::<f32>()..pos_byte_beg + 2 * std::mem::size_of::<f32>()];
                            re = f32::from_be_bytes(re_val.try_into().expect("Could not read be real f32 complex"));
                            im = f32::from_be_bytes(im_val.try_into().expect("Could not read be img f32 complex"));
                            data[i + previous_index] = Complex::new(re, im);
                        }
                    } else {
                        for (i, record) in data_chunk.chunks(record_length).enumerate() {
                            re_val = &record[pos_byte_beg..pos_byte_beg + std::mem::size_of::<f32>()];
                            im_val = &record[pos_byte_beg + std::mem::size_of::<f32>()..pos_byte_beg + 2 * std::mem::size_of::<f32>()];
                            re = f32::from_le_bytes(re_val.try_into().expect("Could not read le real f32 complex"));
                            im = f32::from_le_bytes(im_val.try_into().expect("Could not read le img f32 complex"));
                            data[i + previous_index] = Complex::new(re, im);}
                        }
                    },
                ChannelData::Complex64(data) => {
                    let mut re: f64;
                    let mut im: f64;
                    let mut re_val: &[u8];
                    let mut im_val: &[u8];
                    if cn.endian {
                        for (i, record) in data_chunk.chunks(record_length).enumerate() {
                            re_val = &record[pos_byte_beg..pos_byte_beg + std::mem::size_of::<f64>()];
                            im_val = &record[pos_byte_beg + std::mem::size_of::<f64>()..pos_byte_beg + 2 * std::mem::size_of::<f64>()];
                            re = f64::from_be_bytes(re_val.try_into().expect("Could not array"));
                            im = f64::from_be_bytes(im_val.try_into().expect("Could not array"));
                            data[i + previous_index] = Complex::new(re, im);
                        }
                    } else {
                        for (i, record) in data_chunk.chunks(record_length).enumerate() {
                            re_val = &record[pos_byte_beg..pos_byte_beg + std::mem::size_of::<f64>()];
                            im_val = &record[pos_byte_beg + std::mem::size_of::<f64>()..pos_byte_beg + 2 * std::mem::size_of::<f64>()];
                            re = f64::from_le_bytes(re_val.try_into().expect("Could not read le real f64 complex"));
                            im = f64::from_le_bytes(im_val.try_into().expect("Could not read le img f64 complex"));
                            data[i + previous_index] = Complex::new(re, im);}
                        }
                    },
                ChannelData::StringSBC(data) => {
                    let n_bytes = cn.n_bytes as usize;
                    let mut decoder = WINDOWS_1252.new_decoder();
                    for (i, record) in data_chunk.chunks(record_length).enumerate() {
                        value = &record[pos_byte_beg..pos_byte_beg + n_bytes];
                        let(_result, _size, _replacement) = decoder.decode_to_string(&value, &mut data[i + previous_index], false);}
                    },
                ChannelData::StringUTF8(data) => {
                    let n_bytes = cn.n_bytes as usize;
                    for (i, record) in data_chunk.chunks(record_length).enumerate() {
                        value = &record[pos_byte_beg..pos_byte_beg + n_bytes];
                        data[i + previous_index] = str::from_utf8(&value).expect("Found invalid UTF-8").to_string();}
                    },
                ChannelData::StringUTF16(data) => {
                    let n_bytes = cn.n_bytes as usize;
                    if cn.endian{
                        let mut decoder = UTF_16BE.new_decoder();
                        for (i, record) in data_chunk.chunks(record_length).enumerate() {
                            value = &record[pos_byte_beg..pos_byte_beg + n_bytes];
                            let(_result, _size, _replacement) = decoder.decode_to_string(&value, &mut data[i + previous_index], false);}
                    } else {
                        let mut decoder = UTF_16LE.new_decoder();
                        for (i, record) in data_chunk.chunks(record_length).enumerate() {
                            value = &record[pos_byte_beg..pos_byte_beg + n_bytes];
                            let(_result, _size, _replacement) = decoder.decode_to_string(&value, &mut data[i + previous_index], false);}
                    }},
                ChannelData::ByteArray(data) => {
                    let n_bytes = cn.n_bytes as usize;
                    for (i, record) in data_chunk.chunks(record_length).enumerate() {
                        value = &record[pos_byte_beg..pos_byte_beg + n_bytes];
                        let index = (i + previous_index) * n_bytes;
                        data[index .. index + n_bytes].copy_from_slice(value);}
                    },
            }
        } else if cn.block.cn_type == 5 {
            // Maximum length data channel
            todo!();
        }
        // virtual channels cn_type 3 & 6 are handled at initialisation
        // cn_type == 1 VLSD not possible for sorted data
    })
}

// copies complete sorted data block (not chunk) into each channel array
fn read_all_channels_sorted_from_bytes(data: &[u8], channel_group: &mut Cg4) {
    // initialises the arrays
    initialise_arrays(channel_group, &channel_group.block.cg_cycle_count.clone());
    for nrecord in 0..channel_group.block.cg_cycle_count {
        read_channels_from_bytes(&data[(nrecord * channel_group.record_length as u64) as usize..
            ((nrecord + 1) * channel_group.record_length as u64) as usize], &mut channel_group.cn,  channel_group.record_length as usize, 0);
    }
}

/// Reads unsorted data block chunk by chunk 
fn read_all_channels_unsorted(rdr: &mut BufReader<&File>, dg: &mut Dg4, block_length: i64) {
    let data_block_length = block_length as usize;
    let mut position: usize = 24;
    let mut record_counter: HashMap<u64, (usize, Vec<u8>)> = HashMap::new();
    let mut decoder: Dec = Dec {windows_1252: WINDOWS_1252.new_decoder(), utf_16_be: UTF_16BE.new_decoder(), utf_16_le: UTF_16LE.new_decoder()};
    // initialise record counter that will contain sorted data blocks for each channel group
    for cg in dg.cg.values_mut() {
        record_counter.insert(cg.block.cg_record_id, (0, Vec::new()));
    }

    // reads the sorted data block into chunks
    let mut data_chunk: Vec<u8>;
    while position < data_block_length {
        if (data_block_length - position) > CHUNK_SIZE_READING {
            // not last chunk of data
            data_chunk= vec![0u8; CHUNK_SIZE_READING];
            position += CHUNK_SIZE_READING;
        } else {
            // last chunk of data
            data_chunk= vec![0u8; data_block_length - position];
            position += data_block_length - position;
        }
        rdr.read_exact(&mut data_chunk).expect("Could not read data chunk");
        read_all_channels_unsorted_from_bytes(&mut data_chunk, dg, &mut record_counter, &mut decoder);
    }
}

/// stores a vlsd record into channel vect (ChannelData)
fn save_vlsd(data: &mut ChannelData, record: &[u8], nrecord: &usize, decoder: &mut Dec, endian: bool) {
    match data {
        ChannelData::Int8(_) => {},
        ChannelData::UInt8(_) => {},
        ChannelData::Int16(_) => {},
        ChannelData::UInt16(_) => {},
        ChannelData::Float16(_) => {},
        ChannelData::Int24(_) => {},
        ChannelData::UInt24(_) => {},
        ChannelData::Int32(_) => {},
        ChannelData::UInt32(_) => {},
        ChannelData::Float32(_) => {},
        ChannelData::Int48(_) => {},
        ChannelData::UInt48(_) => {},
        ChannelData::Int64(_) => {},
        ChannelData::UInt64(_) => {},
        ChannelData::Float64(_) => {},
        ChannelData::Complex16(_) => {},
        ChannelData::Complex32(_) => {},
        ChannelData::Complex64(_) => {},
        ChannelData::StringSBC(array) => {
            let(_result, _size, _replacement) = decoder.windows_1252.decode_to_string(&record, &mut array[*nrecord], false);},
        ChannelData::StringUTF8(array) => {
            array[*nrecord] = str::from_utf8(&record).expect("Found invalid UTF-8").to_string();
        },
        ChannelData::StringUTF16(array) => {
            if endian{
                let(_result, _size, _replacement) = decoder.utf_16_be.decode_to_string(&record, &mut array[*nrecord], false);
            } else {
                let(_result, _size, _replacement) = decoder.utf_16_le.decode_to_string(&record, &mut array[*nrecord], false);
            };
        },
        ChannelData::ByteArray(_) => todo!(),
    }
}

/// read record by record from unsorted data block into sorted data block, then copy data into channel arrays 
fn read_all_channels_unsorted_from_bytes(data: &mut Vec<u8>, dg: &mut Dg4, record_counter: &mut HashMap<u64, (usize, Vec<u8>)>, decoder: &mut Dec) {
    let mut position: usize = 0;
    let data_length = data.len();
    // unsort data into sorted data blocks, except for VLSD CG.
    let mut remaining: usize = data_length - position;
    while remaining > 0 {
        // reads record id
        let rec_id: u64;
        let dg_rec_id_size = dg.block.dg_rec_id_size as usize;
        if dg_rec_id_size == 1 && remaining >= 1 {
            rec_id = data[position].try_into().expect("Could not convert record id u8");
        } else if dg_rec_id_size == 2 && remaining >= 2 {
            let rec = &data[position..position + std::mem::size_of::<u16>()];
            rec_id = u16::from_le_bytes(rec.try_into().expect("Could not convert record id u16")) as u64;
        } else if dg_rec_id_size == 4 && remaining >= 4 {
            let rec = &data[position..position + std::mem::size_of::<u32>()];
            rec_id = u32::from_le_bytes(rec.try_into().expect("Could not convert record id u32")) as u64;
        } else if dg_rec_id_size == 8 && remaining >= 8 {
            let rec = &data[position..position + std::mem::size_of::<u64>()];
            rec_id = u64::from_le_bytes(rec.try_into().expect("Could not convert record id u64")) as u64;
        } else {
            break; // not enough data remaining
        }
        // reads record based on record id
        if let Some(cg) = dg.cg.get_mut(&rec_id) {
            let record_length = cg.record_length as usize;
            if remaining >= record_length {
                if (cg.block.cg_flags & 0b1) != 0 {
                    // VLSD channel
                    position += dg_rec_id_size;
                    let len = &data[position..position + std::mem::size_of::<u32>()];
                    let length: usize = u32::from_le_bytes(len.try_into().expect("Could not read length")) as usize;
                    position += std::mem::size_of::<u32>();
                    let record = &data[position..position+ length];
                    if let Some((target_rec_id, target_rec_pos)) = cg.vlsd {
                        if let Some(target_cg) = dg.cg.get_mut(&target_rec_id) {
                            if let Some(target_cn) = target_cg.cn.get_mut(&target_rec_pos) {
                                if let Some((nrecord, _)) = record_counter.get_mut(&rec_id) {
                                    save_vlsd(&mut target_cn.data, record, nrecord, decoder, target_cn.endian);
                                    *nrecord += 1;
                                }
                            }
                        }
                    }
                    position += length;
                } else {
                    // Not VLSD channel
                    let record = &data[position..position + cg.record_length as usize];
                    if let Some((_nrecord, data)) = record_counter.get_mut(&rec_id){
                        data.extend(record);
                    }
                    position += record_length;
                }
            } else {
                break; // not enough data remaining
            }
        }
        remaining = data_length - position;
    }

    // removes consumed records from data and leaves remaining that could not be processed.
    let remaining_vect = data[position..].to_owned();
    data.clear();  // removes data but keeps capacity
    data.extend(remaining_vect);

    // From sorted data block, copies data in channels arrays
    for (rec_id, (index, record_data)) in record_counter.iter_mut() {
        if let Some(channel_group) = dg.cg.get_mut(rec_id) {
            read_channels_from_bytes(&record_data, &mut channel_group.cn, channel_group.record_length as usize, *index);
            record_data.clear(); // clears data for new block, keeping capacity
        }
    }
}

/// decoder for String SBC and UTF16 Le & Be
struct Dec {
    windows_1252: Decoder,
    utf_16_be: Decoder,
    utf_16_le: Decoder,
}

/// initialise ndarrays for the data group/block
fn initialise_arrays(channel_group: &mut Cg4, n_record_chunk: &u64) {
    // creates zeroed array in parallel for each channel contained in channel group
    channel_group.cn.par_iter_mut().for_each(|(_cn_record_position, cn)| {
        cn.data = data_init(cn.block.cn_type, cn.block.cn_data_type, cn.n_bytes, *n_record_chunk);
    })
}

/// applies bit mask if required in channel block
fn apply_bit_mask_offset(dg: &mut Dg4) {
    // apply bit shift and masking
    for channel_group in dg.cg.values_mut() {
        channel_group.cn.par_iter_mut().for_each( |(_rec_pos, cn)| {
            if cn.block.cn_data_type <= 3 {
                let left_shift = cn.n_bytes * 8 - (cn.block.cn_bit_offset as u32) - cn.block.cn_bit_count;
                let right_shift = left_shift + (cn.block.cn_bit_offset as u32);
                if left_shift > 0 || right_shift > 0 {
                    match &mut cn.data {
                        ChannelData::Int8(a) => {
                            if left_shift > 0 {a.map_inplace(|x| *x <<= left_shift)};
                            if right_shift > 0 {a.map_inplace(|x| *x >>= right_shift)};
                        },
                        ChannelData::UInt8(a) => {
                            if left_shift > 0 {a.map_inplace(|x| *x <<= left_shift)};
                            if right_shift > 0 {a.map_inplace(|x| *x >>= right_shift)};
                        },
                        ChannelData::Int16(a) => {
                            if left_shift > 0 {a.map_inplace(|x| *x <<= left_shift)};
                            if right_shift > 0 {a.map_inplace(|x| *x >>= right_shift)};
                        },
                        ChannelData::UInt16(a) => {
                            if left_shift > 0 {a.map_inplace(|x| *x <<= left_shift)};
                            if right_shift > 0 {a.map_inplace(|x| *x >>= right_shift)};
                        },
                        ChannelData::Float16(_) => (),
                        ChannelData::Int24(a) => {
                            if left_shift > 0 {a.map_inplace(|x| *x <<= left_shift)};
                            if right_shift > 0 {a.map_inplace(|x| *x >>= right_shift)};
                        },
                        ChannelData::UInt24(a) => {
                            if left_shift > 0 {a.map_inplace(|x| *x <<= left_shift)};
                            if right_shift > 0 {a.map_inplace(|x| *x >>= right_shift)};
                        },
                        ChannelData::Int32(a) => {
                            if left_shift > 0 {a.map_inplace(|x| *x <<= left_shift)};
                            if right_shift > 0 {a.map_inplace(|x| *x >>= right_shift)};
                        },
                        ChannelData::UInt32(a) => {
                            if left_shift > 0 {a.map_inplace(|x| *x <<= left_shift)};
                            if right_shift > 0 {a.map_inplace(|x| *x >>= right_shift)};
                        },
                        ChannelData::Float32(_) => (),
                        ChannelData::Int48(a) => {
                            if left_shift > 0 {a.map_inplace(|x| *x <<= left_shift)};
                            if right_shift > 0 {a.map_inplace(|x| *x >>= right_shift)};
                        },
                        ChannelData::UInt48(a) => {
                            if left_shift > 0 {a.map_inplace(|x| *x <<= left_shift)};
                            if right_shift > 0 {a.map_inplace(|x| *x >>= right_shift)};
                        },
                        ChannelData::Int64(a) => {
                            if left_shift > 0 {a.map_inplace(|x| *x <<= left_shift)};
                            if right_shift > 0 {a.map_inplace(|x| *x >>= right_shift)};
                        },
                        ChannelData::UInt64(a) => {
                            if left_shift > 0 {a.map_inplace(|x| *x <<= left_shift)};
                            if right_shift > 0 {a.map_inplace(|x| *x >>= right_shift)};
                        },
                        ChannelData::Float64(_) => (),
                        ChannelData::Complex16(_) => (),
                        ChannelData::Complex32(_) => (),
                        ChannelData::Complex64(_) => (),
                        ChannelData::StringSBC(_) => (),
                        ChannelData::StringUTF8(_) => (),
                        ChannelData::StringUTF16(_) => (),
                        ChannelData::ByteArray(_) => (),
                    }
                }
            }
        })
    }
}

/// channel data type enum
#[derive(Debug, Clone)]
pub enum ChannelData {
    Int8(Array1<i8>),
    UInt8(Array1<u8>),
    Int16(Array1<i16>),
    UInt16(Array1<u16>),
    Float16(Array1<f32>),
    Int24(Array1<i32>),
    UInt24(Array1<u32>),
    Int32(Array1<i32>),
    UInt32(Array1<u32>),
    Float32(Array1<f32>),
    Int48(Array1<i64>),
    UInt48(Array1<u64>),
    Int64(Array1<i64>),
    UInt64(Array1<u64>),
    Float64(Array1<f64>),
    Complex16(Array1<Complex<f32>>),
    Complex32(Array1<Complex<f32>>),
    Complex64(Array1<Complex<f64>>),
    StringSBC(Vec<String>),
    StringUTF8(Vec<String>),
    StringUTF16(Vec<String>),
    ByteArray(Vec<u8>),
}

impl ChannelData {
    pub fn zeros(&self, cycle_count: u64, n_bytes: u32) -> ChannelData {
        match self {
            ChannelData::Int8(_) => ChannelData::Int8(ArrayBase::<OwnedRepr<i8>, Dim<[usize; 1]>>::zeros((cycle_count as usize,))),
            ChannelData::UInt8(_) => ChannelData::UInt8(ArrayBase::<OwnedRepr<u8>, Dim<[usize; 1]>>::zeros((cycle_count as usize,))),
            ChannelData::Int16(_) => ChannelData::Int16(ArrayBase::<OwnedRepr<i16>, Dim<[usize; 1]>>::zeros((cycle_count as usize,))),
            ChannelData::UInt16(_) => ChannelData::UInt16(ArrayBase::<OwnedRepr<u16>, Dim<[usize; 1]>>::zeros((cycle_count as usize,))),
            ChannelData::Float16(_) => ChannelData::Float16(ArrayBase::<OwnedRepr<f32>, Dim<[usize; 1]>>::zeros((cycle_count as usize,))),
            ChannelData::Int24(_) => ChannelData::Int24(ArrayBase::<OwnedRepr<i32>, Dim<[usize; 1]>>::zeros((cycle_count as usize,))),
            ChannelData::UInt24(_) => ChannelData::UInt24(ArrayBase::<OwnedRepr<u32>, Dim<[usize; 1]>>::zeros((cycle_count as usize,))),
            ChannelData::Int32(_) => ChannelData::Int32(ArrayBase::<OwnedRepr<i32>, Dim<[usize; 1]>>::zeros((cycle_count as usize,))),
            ChannelData::UInt32(_) => ChannelData::UInt32(ArrayBase::<OwnedRepr<u32>, Dim<[usize; 1]>>::zeros((cycle_count as usize,))),
            ChannelData::Float32(_) => ChannelData::Float32(ArrayBase::<OwnedRepr<f32>, Dim<[usize; 1]>>::zeros((cycle_count as usize,))),
            ChannelData::Int48(_) => ChannelData::Int48(ArrayBase::<OwnedRepr<i64>, Dim<[usize; 1]>>::zeros((cycle_count as usize,))),
            ChannelData::UInt48(_) => ChannelData::UInt48(ArrayBase::<OwnedRepr<u64>, Dim<[usize; 1]>>::zeros((cycle_count as usize,))),
            ChannelData::Int64(_) => ChannelData::Int64(ArrayBase::<OwnedRepr<i64>, Dim<[usize; 1]>>::zeros((cycle_count as usize,))),
            ChannelData::UInt64(_) => ChannelData::UInt64(ArrayBase::<OwnedRepr<u64>, Dim<[usize; 1]>>::zeros((cycle_count as usize,))),
            ChannelData::Float64(_) => ChannelData::Float64(ArrayBase::<OwnedRepr<f64>, Dim<[usize; 1]>>::zeros((cycle_count as usize,))),
            ChannelData::Complex16(_) => ChannelData::Complex16(ArrayBase::<OwnedRepr<Complex<f32>>, Dim<[usize; 1]>>::zeros((cycle_count as usize,))),
            ChannelData::Complex32(_) => ChannelData::Complex32(ArrayBase::<OwnedRepr<Complex<f32>>, Dim<[usize; 1]>>::zeros((cycle_count as usize,))),
            ChannelData::Complex64(_) => ChannelData::Complex64(ArrayBase::<OwnedRepr<Complex<f64>>, Dim<[usize; 1]>>::zeros((cycle_count as usize,))),
            ChannelData::StringSBC(_) => ChannelData::StringSBC(vec![String::new(); cycle_count as usize]),
            ChannelData::StringUTF8(_) => ChannelData::StringUTF8(vec![String::new(); cycle_count as usize]),
            ChannelData::StringUTF16(_) => ChannelData::StringUTF16(vec![String::new(); cycle_count as usize]),
            ChannelData::ByteArray(_) => ChannelData::ByteArray(vec![0u8; (n_bytes as u64 * cycle_count) as usize]),
        }
    }
}

impl Default for ChannelData {
    fn default() -> Self { ChannelData::UInt8(Array1::<u8>::zeros((0, ))) }
}

/// Initialises a channel array with cycle_count zeroes and correct depending of cn_type, cn_data_type and number of bytes
pub fn data_init(cn_type: u8, cn_data_type: u8, n_bytes: u32, cycle_count: u64) -> ChannelData {
    let data_type: ChannelData;
    if cn_type != 3 || cn_type != 6 {
        if cn_data_type == 0 || cn_data_type == 1 {
            // unsigned int
            if n_bytes <= 1 {
                data_type = ChannelData::UInt8(Array1::<u8>::zeros((cycle_count as usize, )));
            } else if n_bytes == 2 {
                data_type = ChannelData::UInt16(Array1::<u16>::zeros((cycle_count as usize, )));
            } else if n_bytes == 3 {
                data_type = ChannelData::UInt24(Array1::<u32>::zeros((cycle_count as usize, )));
            } else if n_bytes == 4 {
                data_type = ChannelData::UInt32(Array1::<u32>::zeros((cycle_count as usize, )));
            } else if n_bytes <= 6 {
                data_type = ChannelData::UInt48(Array1::<u64>::zeros((cycle_count as usize, )));
            } else {
                data_type = ChannelData::UInt64(Array1::<u64>::zeros((cycle_count as usize, )));
            }
        } else if cn_data_type == 2 || cn_data_type == 3 {
            // signed int
            if n_bytes <= 1 {
                data_type = ChannelData::Int8(Array1::<i8>::zeros((cycle_count as usize, )));
            } else if n_bytes == 2 {
                data_type = ChannelData::Int16(Array1::<i16>::zeros((cycle_count as usize, )));
            }  else if n_bytes == 3 {
                data_type = ChannelData::Int24(Array1::<i32>::zeros((cycle_count as usize, )));
            } else if n_bytes == 4 {
                data_type = ChannelData::Int32(Array1::<i32>::zeros((cycle_count as usize, )));
            } else if n_bytes <= 6 {
                data_type = ChannelData::Int48(Array1::<i64>::zeros((cycle_count as usize, )));
            }else {
                data_type = ChannelData::Int64(Array1::<i64>::zeros((cycle_count as usize, )));
            }
        } else if cn_data_type == 4 || cn_data_type == 5 {
            // float
            if n_bytes <= 2 {
                data_type = ChannelData::Float16(Array1::<f32>::zeros((cycle_count as usize, )));
            } else if n_bytes <= 4 {
                data_type = ChannelData::Float32(Array1::<f32>::zeros((cycle_count as usize, )));
            } else {
                data_type = ChannelData::Float64(Array1::<f64>::zeros((cycle_count as usize, )));
            } 
        } else if cn_data_type == 15 || cn_data_type == 16 {
            // complex
            if n_bytes <= 2 {
                data_type = ChannelData::Complex16(Array1::<Complex<f32>>::zeros((cycle_count as usize, )));
            } else if n_bytes <= 4 {
                data_type = ChannelData::Complex32(Array1::<Complex<f32>>::zeros((cycle_count as usize, )));
            } else {
                data_type = ChannelData::Complex64(Array1::<Complex<f64>>::zeros((cycle_count as usize, )));
            } 
        } else if cn_data_type == 6 {
            // SBC ISO-8859-1 to be converted into UTF8
            data_type = ChannelData::StringSBC(vec![String::new(); cycle_count as usize]);
        } else if cn_data_type == 7 {
            // String UTF8
            data_type = ChannelData::StringUTF8(vec![String::new(); cycle_count as usize]);
        } else if cn_data_type == 8 || cn_data_type == 9 {
            // String UTF16 to be converted into UTF8
            data_type = ChannelData::StringUTF16(vec![String::new(); cycle_count as usize]);
        } else {
            // bytearray
            data_type = ChannelData::ByteArray(vec![0u8; (n_bytes as u64 * cycle_count) as usize]);
        }
    } else {
        // virtual channels, cn_bit_count = 0 -> n_bytes = 0, must be LE unsigned int
        data_type = ChannelData::UInt64(Array1::<u64>::from_iter(0..cycle_count));
    }
    data_type
}

/// convert all channel arrays into physical values as required by CCBlock content
fn convert_all_channels(dg: &mut Dg4, cc: &HashMap<i64, Cc4Block>) {
    for channel_group in dg.cg.values_mut() {
        for (_cn_record_position, cn) in channel_group.cn.iter_mut() {
            if let Some(conv) = cc.get(&cn.block.cn_cc_conversion) {
                match conv.cc_type {
                    1 => linear_conversion(cn, &conv.cc_val, &channel_group.block.cg_cycle_count),
                    2 => rational_conversion(cn, &conv.cc_val, &channel_group.block.cg_cycle_count),
                    _ => {},
                }
            }
        }
    }
}

/// Apply linear conversion to get physical data
fn linear_conversion(cn: &mut Cn4, cc_val: &[f64], cycle_count: &u64) {
    let p1 = cc_val[0];
    let p2 = cc_val[1];
    if !(p1 == 0.0 && (p2 - 1.0) < 1e-12) {
        let mut new_array = Array1::<f64>::zeros((*cycle_count as usize,));
        match &mut cn.data {
            ChannelData::UInt8(a) => {
                Zip::from(&mut new_array).and(a).par_for_each(|new_array, a| *new_array = (*a as f64) * p2 + p1);
                cn.data =  ChannelData::Float64(new_array);
                },
            ChannelData::Int8(a) => {
                Zip::from(&mut new_array).and(a).par_for_each(|new_array, a| *new_array = (*a as f64) * p2 + p1);
                cn.data =  ChannelData::Float64(new_array);
                },
            ChannelData::Int16(a) => {
                Zip::from(&mut new_array).and(a).par_for_each(|new_array, a| *new_array = (*a as f64) * p2 + p1);
                cn.data =  ChannelData::Float64(new_array);
                },
            ChannelData::UInt16(a) => {
                Zip::from(&mut new_array).and(a).par_for_each(|new_array, a| *new_array = (*a as f64) * p2 + p1);
                cn.data =  ChannelData::Float64(new_array);
                },
            ChannelData::Float16(a) => {
                Zip::from(&mut new_array).and(a).par_for_each(|new_array, a| *new_array = (*a as f64) * p2 + p1);
                cn.data =  ChannelData::Float64(new_array);
                },
            ChannelData::Int24(a) => {
                Zip::from(&mut new_array).and(a).par_for_each(|new_array, a| *new_array = (*a as f64) * p2 + p1);
                cn.data =  ChannelData::Float64(new_array);
                },
            ChannelData::UInt24(a) => {
                Zip::from(&mut new_array).and(a).par_for_each(|new_array, a| *new_array = (*a as f64) * p2 + p1);
                cn.data =  ChannelData::Float64(new_array);
                },
            ChannelData::Int32(a) => {
                Zip::from(&mut new_array).and(a).par_for_each(|new_array, a| *new_array = (*a as f64) * p2 + p1);
                cn.data =  ChannelData::Float64(new_array);
                },
            ChannelData::UInt32(a) => {
                Zip::from(&mut new_array).and(a).par_for_each(|new_array, a| *new_array = (*a as f64) * p2 + p1);
                cn.data =  ChannelData::Float64(new_array);
                },
            ChannelData::Float32(a) => {
                Zip::from(&mut new_array).and(a).par_for_each(|new_array, a| *new_array = (*a as f64) * p2 + p1);
                cn.data =  ChannelData::Float64(new_array);
                },
            ChannelData::Int48(a) => {
                Zip::from(&mut new_array).and(a).par_for_each(|new_array, a| *new_array = (*a as f64) * p2 + p1);
                cn.data =  ChannelData::Float64(new_array);
                },
            ChannelData::UInt48(a) => {
                Zip::from(&mut new_array).and(a).par_for_each(|new_array, a| *new_array = (*a as f64) * p2 + p1);
                cn.data =  ChannelData::Float64(new_array);
                },
            ChannelData::Int64(a) => {
                Zip::from(&mut new_array).and(a).par_for_each(|new_array, a| *new_array = (*a as f64) * p2 + p1);
                cn.data =  ChannelData::Float64(new_array);
                },
            ChannelData::UInt64(a) => {
                Zip::from(&mut new_array).and(a).par_for_each(|new_array, a| *new_array = (*a as f64) * p2 + p1);
                cn.data =  ChannelData::Float64(new_array);
                },
            ChannelData::Float64(a) => {
                Zip::from(&mut new_array).and(a).par_for_each(|new_array, a| *new_array = *a * p2 + p1);
                cn.data =  ChannelData::Float64(new_array);
            },
            ChannelData::Complex16(_) => todo!(),
            ChannelData::Complex32(_) => todo!(),
            ChannelData::Complex64(_) => todo!(),
            ChannelData::StringSBC(_) => {},
            ChannelData::StringUTF8(_) => {},
            ChannelData::StringUTF16(_) => {},
            ChannelData::ByteArray(_) => {},
        }
    }
}

// Apply rational conversion to get physical data
fn rational_conversion(cn: &mut Cn4, cc_val: &[f64], cycle_count: &u64) {
    let p1 = cc_val[0];
    let p2 = cc_val[1];
    let p3 = cc_val[2];
    let p4 = cc_val[3];
    let p5 = cc_val[4];
    let p6 = cc_val[5];
    let mut new_array = Array1::<f64>::zeros((*cycle_count as usize,));
    match &mut cn.data {
        ChannelData::UInt8(a) => {
            Zip::from(&mut new_array).and(a).par_for_each(|new_array, a| {
                let m = *a as f64;
                let m_2 = f64::powi(m, 2);
                *new_array = (m_2 * p1 + m * p2 + p3) / (m_2 * p4 + m * p5 + p6)});
            cn.data =  ChannelData::Float64(new_array);
            },
        ChannelData::Int8(a) => {
            Zip::from(&mut new_array).and(a).par_for_each(|new_array, a| {
                let m = *a as f64;
                let m_2 = f64::powi(m, 2);
                *new_array = (m_2 * p1 + m * p2 + p3) / (m_2 * p4 + m * p5 + p6)});
            cn.data =  ChannelData::Float64(new_array);
            },
        ChannelData::Int16(a) => {
            Zip::from(&mut new_array).and(a).par_for_each(|new_array, a| {
                let m = *a as f64;
                let m_2 = f64::powi(m, 2);
                *new_array = (m_2 * p1 + m * p2 + p3) / (m_2 * p4 + m * p5 + p6)});
            cn.data =  ChannelData::Float64(new_array);
            },
        ChannelData::UInt16(a) => {
            Zip::from(&mut new_array).and(a).par_for_each(|new_array, a| {
                let m = *a as f64;
                let m_2 = f64::powi(m, 2);
                *new_array = (m_2 * p1 + m * p2 + p3) / (m_2 * p4 + m * p5 + p6)});
            cn.data =  ChannelData::Float64(new_array);
            },
        ChannelData::Float16(a) => {
            Zip::from(&mut new_array).and(a).par_for_each(|new_array, a| {
                let m = *a as f64;
                let m_2 = f64::powi(m, 2);
                *new_array = (m_2 * p1 + m * p2 + p3) / (m_2 * p4 + m * p5 + p6)});
            cn.data =  ChannelData::Float64(new_array);
            },
        ChannelData::Int24(a) => {
            Zip::from(&mut new_array).and(a).par_for_each(|new_array, a| {
                let m = *a as f64;
                let m_2 = f64::powi(m, 2);
                *new_array = (m_2 * p1 + m * p2 + p3) / (m_2 * p4 + m * p5 + p6)});
            cn.data =  ChannelData::Float64(new_array);
            },
        ChannelData::UInt24(a) => {
            Zip::from(&mut new_array).and(a).par_for_each(|new_array, a| {
                let m = *a as f64;
                let m_2 = f64::powi(m, 2);
                *new_array = (m_2 * p1 + m * p2 + p3) / (m_2 * p4 + m * p5 + p6)});
            cn.data =  ChannelData::Float64(new_array);
            },
        ChannelData::Int32(a) => {
            Zip::from(&mut new_array).and(a).par_for_each(|new_array, a| {
                let m = *a as f64;
                let m_2 = f64::powi(m, 2);
                *new_array = (m_2 * p1 + m * p2 + p3) / (m_2 * p4 + m * p5 + p6)});
            cn.data =  ChannelData::Float64(new_array);
            },
        ChannelData::UInt32(a) => {
            Zip::from(&mut new_array).and(a).par_for_each(|new_array, a| {
                let m = *a as f64;
                let m_2 = f64::powi(m, 2);
                *new_array = (m_2 * p1 + m * p2 + p3) / (m_2 * p4 + m * p5 + p6)});
            cn.data =  ChannelData::Float64(new_array);
            },
        ChannelData::Float32(a) => {
            Zip::from(&mut new_array).and(a).par_for_each(|new_array, a| {
                let m = *a as f64;
                let m_2 = f64::powi(m, 2);
                *new_array = (m_2 * p1 + m * p2 + p3) / (m_2 * p4 + m * p5 + p6)});
            cn.data =  ChannelData::Float64(new_array);
            },
        ChannelData::Int48(a) => {
            Zip::from(&mut new_array).and(a).par_for_each(|new_array, a| {
                let m = *a as f64;
                let m_2 = f64::powi(m, 2);
                *new_array = (m_2 * p1 + m * p2 + p3) / (m_2 * p4 + m * p5 + p6)});
            cn.data =  ChannelData::Float64(new_array);
            },
        ChannelData::UInt48(a) => {
            Zip::from(&mut new_array).and(a).par_for_each(|new_array, a| {
                let m = *a as f64;
                let m_2 = f64::powi(m, 2);
                *new_array = (m_2 * p1 + m * p2 + p3) / (m_2 * p4 + m * p5 + p6)});
            cn.data =  ChannelData::Float64(new_array);
            },
        ChannelData::Int64(a) => {
            Zip::from(&mut new_array).and(a).par_for_each(|new_array, a| {
                let m = *a as f64;
                let m_2 = f64::powi(m, 2);
                *new_array = (m_2 * p1 + m * p2 + p3) / (m_2 * p4 + m * p5 + p6)});
            cn.data =  ChannelData::Float64(new_array);
            },
        ChannelData::UInt64(a) => {
            Zip::from(&mut new_array).and(a).par_for_each(|new_array, a| {
                let m = *a as f64;
                let m_2 = f64::powi(m, 2);
                *new_array = (m_2 * p1 + m * p2 + p3) / (m_2 * p4 + m * p5 + p6)});
            cn.data =  ChannelData::Float64(new_array);
            },
        ChannelData::Float64(a) => {
            Zip::from(&mut new_array).and(a).par_for_each(|new_array, a| {
                let m_2 = f64::powi(*a, 2);
                *new_array = (m_2 * p1 + *a * p2 + p1) / (m_2 * p4 + *a * p5 + p6)});
            cn.data =  ChannelData::Float64(new_array);
        },
        ChannelData::Complex16(_) => todo!(),
        ChannelData::Complex32(_) => todo!(),
        ChannelData::Complex64(_) => todo!(),
        ChannelData::StringSBC(_) => {},
        ChannelData::StringUTF8(_) => {},
        ChannelData::StringUTF16(_) => {},
        ChannelData::ByteArray(_) => {},
    }
}