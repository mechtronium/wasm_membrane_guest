pub mod error;

use wasm_bindgen::prelude::*;
use wasm_bindgen::__rt::std::alloc::{Layout,alloc,dealloc};
use wasm_bindgen::__rt::std::mem;
use mechtron_common::core::*;
use wasm_bindgen::__rt::core::slice;
use std::borrow::BorrowMut;
use std::sync::{RwLock, Mutex, MutexGuard,Arc};
use wasm_bindgen::__rt::std::collections::HashMap;
use wasm_bindgen::__rt::std::sync::atomic::{Ordering, AtomicPtr};
use wasm_bindgen::__rt::std::sync::atomic::AtomicI32;
use crate::CONFIGS;
use crate::mechtron;
use std::ops::{Deref, DerefMut};
use mechtron_common::state::{State, NeutronStateInterface, StateMeta};
use mechtron_common::error::Error;
use mechtron_common::id::{Id, MechtronKey};
use mechtron_common::message::{Message, MessageBuilder};
use mechtron_common::mechtron::Context;
use mechtron_common::buffers::Buffer;
use crate::mechtron::{MessageHandler, Response};
use std::rc::Rc;
use std::cell::{Cell, RefCell};
use mechtron_common::artifact::Artifact;
use crate::error::Error;
use std::collections::HashSet;

lazy_static! {
  pub static ref BUFFERS: RwLock<HashMap<i32,Vec<u8>>> = RwLock::new(HashMap::new());
  pub static ref BUFFER_INDEX: AtomicI32 = AtomicI32::new(0);
  pub static ref EXTENSIONS : RwLock<HashSet>= HashSet::new();
}

pub static OK    : i32 = 0;
pub static ERROR : i32 = -1;
pub static EMPTY : i32 = -2;


extern "C"
{
    pub fn membrane_guest_init();
    pub fn membrane_host_log( string_buffer_id: i32 );
    pub fn membrane_host_panic( string_buffer_id: i32);
    pub fn membrane_host_test_buffer_callback( buffer_id: i32);
    pub fn membrane_host_test_string_callback( buffer_id: i32);
    pub fn membrane_host_test_log_callback( );
    pub fn membrane_host_test_panic_callback( );
}

#[wasm_bindgen]
pub fn membrane_guest_version()->i32
{
    return 0;
}

//////////////////////////////////////
// methods used by guest to init
//////////////////////////////////////

pub fn membrane_init_add_ext(ext:&str){
   let exts = EXTENSIONS.write().unwrap();
   exts.insert(ext.to_string());
}

//////////////////////////////////////
// methods used by guest to manage buffers
//////////////////////////////////////
pub fn membrane_buffer(mut bytes: Vec<u8>) -> i32{
    let mut buffers = BUFFERS.write().unwrap();
    let buffer_id = BUFFER_INDEX.fetch_add(1, Ordering::Relaxed );
    buffers.insert(buffer_id, bytes );
    buffer_id
}

pub fn membrane_consume_buffer(buffer_id: i32) -> Result<Vec<u8>, Error>
{
    let bytes: Option<Vec<u8>> = {
        let mut buffers = BUFFERS.write()?;
        buffers.remove(&buffer_id)
    };
    match bytes{
        None => Err(format!("could not find buffer: {}",buffer_id).into()),
        Some(bytes) => Ok(bytes)
    }
}

pub fn membrane_consume_string_utf8(buffer_id: i32) -> Result<String, Error>
{
    let mut buffers = BUFFERS.write()?;
    let bytes = buffers.remove(&buffer_id).unwrap().to_vec();
    Ok(String::from_utf8(bytes)?)
}

pub fn membrane_string_utf8(mut string: String) -> i32{
    membrane_buffer(string.into_bytes() )
}

//////////////////////////////////////
// ext access from host
//////////////////////////////////////
#[wasm_bindgen]
pub fn membrane_get_extensions() -> i32
{
    let exts = {
        let exts: HashSet<String> = EXTENSIONS.read().unwrap();
        let mut builder = String::new();
        for ext in exts
        {
            builder.push_str(ext.as_str());
            builder.push_str(",");
        }
        builder
    };

    let string_id = membrane_string_utf8(exts);
    string_id
}

//////////////////////////////////////
// buffer access from host
//////////////////////////////////////

#[wasm_bindgen]
pub fn membrane_get_buffer_ptr(id: i32) ->*const u8
{
    let buffer_info = BUFFERS.read();
    let buffer_info = buffer_info.unwrap();
    let buffer = buffer_info.get(&id).unwrap();
    return buffer.as_ptr()
}

#[wasm_bindgen]
pub fn membrane_get_buffer_len(id: i32) ->i32
{
    let buffer_info = BUFFERS.read();
    let buffer_info = buffer_info.unwrap();
    let buffer = buffer_info.get(&id).unwrap();
    buffer.len() as _

}

#[wasm_bindgen]
pub fn membrane_alloc_buffer(len: i32) ->i32
{
    let buffer_id = BUFFER_INDEX.fetch_add(1, Ordering::Relaxed);
    {
        let mut buffers = BUFFERS.write().unwrap();
        let mut bytes: Vec<u8> = Vec::with_capacity(len as _);
        unsafe { bytes.set_len(len as _) }
        buffers.insert(buffer_id, bytes);
    }
    buffer_id
}

#[wasm_bindgen]
pub fn membrane_dealloc_buffer(id: i32)
{
    let mut buffers= BUFFERS.write().unwrap();
    buffers.remove( &id );
}

/////////////////////////////////////////
// convenience methods
/////////////////////////////////////////

pub fn panic( message: String )
{
    let buffer_id = membrane_string(message);
    unsafe {
        membrane_host_panic(buffer_id );
    }
}

pub fn log( message: String )
{
    let buffer_id = membrane_string(message);
    unsafe {
        membrane_host_log(buffer_id );
    }
}

/////////////////////////////////////////
// tests executed by host to see if membrane is working
/////////////////////////////////////////

#[wasm_bindgen]
pub fn membrane_guest_test_buffer_callback(buffer_id: i32)->i32
{
    let buffer = match membrane_consume_buffer(buffer_id){
        Ok(buffer) => {
            buffer
        }
        Err(error) => {
            return ERROR;
        }
    };
    // we create a new buffer
    let buffer_id = membrane_buffer(buffer);
    // and then we report that buffer to the host
    unsafe
        {
            membrane_host_test_buffer_callback(buffer_id);
        }
    OK
}

#[wasm_bindgen]
pub fn membrane_guest_test_string_callback(buffer_id: i32)->i32
{
    let buffer = match membrane_consume_string_utf8(buffer_id){
        Ok(buffer) => {
            buffer
        }
        Err(error) => {
            return ERROR;
        }
    };
    // we create a new buffer
    let buffer_id = membrane_string_utf8(buffer);
    // and then we report that buffer to the host
    unsafe
        {
            membrane_host_test_string_callback(buffer_id);
        }
    OK
}

#[wasm_bindgen]
pub fn membrane_guest_test_log_callback(buffer_id: i32)->i32
{
    let message = match membrane_consume_string_utf8(buffer_id){
        Ok(buffer) => {
            buffer
        }
        Err(error) => {
            return ERROR;
        }
    };

    log(message);

    OK
}

#[wasm_bindgen]
pub fn membrane_guest_test_panic_callback(buffer_id: i32)->i32
{
    let message = match membrane_consume_string_utf8(buffer_id){
        Ok(buffer) => {
            buffer
        }
        Err(error) => {
            return ERROR;
        }
    };

    panic(message);

    OK
}

