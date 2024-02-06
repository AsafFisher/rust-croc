// I hate this.

use anyhow::Result;
use num_bigint_dig::{BigInt, ModInverse};
use num_traits::{PrimInt, Zero};
use once_cell::sync::Lazy;
use rand::RngCore;
use serde::{self, Deserialize, Deserializer, Serialize, Serializer};

use serde_repr::{Deserialize_repr, Serialize_repr};
use serde_with::{serde_as, DeserializeAs, SerializeAs};

use sha2::{Digest, Sha256};
use std::convert::TryInto;
use std::default::Default;

use std::io::Write;
use std::str::FromStr;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error("Key sent has the same role as this instance")]
    SameRole,
    #[error("The given point 'Point(x: {x:?}, y: {y:?})' is not on the eliptic curve")]
    PointNotOnCurve { x: BigInt, y: BigInt },
    #[error("The given point 'Point(x: {x:?}, y: {y:?})' is invalid")]
    InvalidPoint {
        x: Option<BigInt>,
        y: Option<BigInt>,
    },
    #[error("Fatal, the alpha is missing")]
    InvalidAlpha,
}
pub trait EllipticCurve {
    fn add(&self, x1: &BigInt, y1: &BigInt, x2: &BigInt, y2: &BigInt) -> (BigInt, BigInt);
    fn scalar_base_mult(&self, k: &[u8]) -> (BigInt, BigInt);
    fn scalar_mult(&self, bx: &BigInt, by: &BigInt, k: &[u8]) -> (BigInt, BigInt);
    fn is_on_curve(&self, x: &BigInt, y: &BigInt) -> bool;
}

pub struct SIEC255Params {
    // the order of the underlying field
    p: BigInt,
    // the order of the base point
    n: BigInt,
    // the other constant of the curve equation
    a: BigInt,
    // the constant of the curve equation
    b: BigInt,
    // (x,y) of the base point
    gx: BigInt,
    gy: BigInt,
    // the size of the underlying field
    bit_size: usize,
    // the canonical name of the curve
    name: String,
}

impl Default for SIEC255Params {
    fn default() -> Self {
        SIEC255Params {
            name: String::from("SIEC255"),
            gx: BigInt::from(5u32),
            gy: BigInt::from(12u32),
            a: BigInt::from(0u32),
            b: BigInt::from(19u32),
            p: BigInt::from_str(
                "28948022309329048855892746252183396360603931420023084536990047309120118726721",
            )
            .unwrap(),
            n: BigInt::from_str(
                "28948022309329048855892746252183396360263649053102146073526672701688283398081",
            )
            .unwrap(),
            bit_size: 255,
        }
    }
}

impl SIEC255Params {
    // Double returns 2*(x,y)
    fn double(&self, x1: &BigInt, y1: &BigInt) -> (BigInt, BigInt) {
        let mut x3 = x1.clone().modpow(&2.into(), &self.p);

        // λ = (3x1^2)/(2y1)
        let mut lambda: BigInt = x3 * 3;
        if y1.bits() == 0 {
            return (BigInt::zero(), BigInt::zero());
        }

        x3 = 2 * y1;
        x3 = x3.mod_inverse(&self.p).unwrap();
        lambda *= &x3;
        // x3 = λ² - x1 - x2
        x3 = lambda.modpow(&2.into(), &self.p);
        let mut y3 = x1 + x1;
        x3 = x3 - y3;
        x3 = x3.modpow(&1.into(), &self.p);

        // y3 = λ(x1 - x3) - y1
        y3 = &lambda * &(x1 - &x3);
        y3 = y3.modpow(&1.into(), &self.p);
        y3 -= y1;
        y3 = y3.modpow(&1.into(), &self.p);
        (x3, y3)
    }
}
impl EllipticCurve for SIEC255Params {
    fn add(&self, x1: &BigInt, y1: &BigInt, x2: &BigInt, y2: &BigInt) -> (BigInt, BigInt) {
        if x1.bits() == 0 && y1.bits() == 0 {
            (x2.clone(), y2.clone())
        } else if x2.bits() == 0 && y2.bits() == 0 {
            (x1.clone(), y1.clone())
        } else if x1 == x2 && y1 == y2 {
            self.double(&x1, &y1)
        } else {
            // λ = (y2 - y1)/(x2 - x1)
            let z = x2 - x1;
            let mut lambda = y2 - y1;
            if z.bits() == 0 {
                return (Zero::zero(), Zero::zero());
            }
            let z = z.mod_inverse(&self.p).unwrap();
            lambda = lambda * z;
            lambda = lambda.modpow(&1.into(), &self.p);

            // x3 = λ² - x1 - x2
            let mut x3 = lambda.modpow(&2.into(), &self.p);
            x3 = x3 - (x1 + x2);
            x3 = x3.modpow(&1.into(), &self.p);

            // y3 = λ(x1 - x3) - y1
            let mut y3 = (lambda * (x1 - &x3)).modpow(&1.into(), &self.p);
            y3 = y3 - y1;
            y3 = y3.modpow(&1.into(), &self.p);

            (x3, y3)
        }
    }

    fn scalar_base_mult(&self, k: &[u8]) -> (BigInt, BigInt) {
        self.scalar_mult(&self.gx, &self.gy, k)
    }

    fn scalar_mult(&self, x1: &BigInt, y1: &BigInt, k: &[u8]) -> (BigInt, BigInt) {
        let (mut x, mut y) = (BigInt::zero(), BigInt::zero());
        for b in k {
            let mut cur_b = *b;
            for _ in 0..8 {
                (x, y) = self.double(&x, &y);
                if cur_b & 0x80 == 0x80 {
                    (x, y) = self.add(&x1, &y1, &x, &y);
                }
                cur_b = cur_b.unsigned_shl(1);
            }
        }
        (x, y)
    }

    fn is_on_curve(&self, x: &BigInt, y: &BigInt) -> bool {
        // y² = x³ + 19
        let nineteen = BigInt::from(19u32);
        let lhs = y.modpow(&BigInt::from(2), &self.p);
        let rhs = x.modpow(&BigInt::from(3), &self.p) + nineteen;
        let rhs = rhs.modpow(&1.into(), &self.p);

        lhs == rhs
    }
}
static SIEC255: Lazy<SIEC255Params> = Lazy::new(|| Default::default());

fn siec255() -> &'static SIEC255Params {
    &SIEC255
}

#[derive(Serialize_repr, Deserialize_repr, PartialEq, Clone, Copy, Debug)]
#[repr(u8)]
pub enum Role {
    Sender,
    Reciever,
}

mod precision_integer {
    use num_bigint_dig::BigInt;
    use serde::{de, ser, Deserialize, Deserializer, Serialize, Serializer};
    use serde_json::Number;
    use std::str::FromStr;

    pub fn serialize<T: ToString, S: Serializer>(
        value: &T,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        Number::from_str(&value.to_string())
            .map_err(|err| -> S::Error {
                ser::Error::custom(format!("Could not serialize bigint {err}"))
            })?
            .serialize(serializer)
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<BigInt, D::Error> {
        BigInt::from_str(&Number::deserialize(deserializer)?.to_string())
            .map_err(|err| de::Error::custom(format!("Could not deserialize bigint {err}")))
    }
}

struct NumberFromString;

impl SerializeAs<BigInt> for NumberFromString {
    fn serialize_as<S>(value: &BigInt, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        precision_integer::serialize(value, serializer)
    }
}

impl<'de> DeserializeAs<'de, BigInt> for NumberFromString {
    fn deserialize_as<D>(deserializer: D) -> Result<BigInt, D::Error>
    where
        D: Deserializer<'de>,
    {
        precision_integer::deserialize(deserializer)
    }
}

#[serde_as]
#[derive(Serialize, Deserialize, Debug)]
pub struct PakePubKey {
    #[serde(rename = "Role")]
    role: Role,
    #[serde(rename = "Uᵤ", with = "precision_integer")]
    u_u: BigInt,
    #[serde(rename = "Uᵥ", with = "precision_integer")]
    u_v: BigInt,
    #[serde(rename = "Vᵤ", with = "precision_integer")]
    v_u: BigInt,
    #[serde(rename = "Vᵥ", with = "precision_integer")]
    v_v: BigInt,
    #[serde_as(as = "Option<NumberFromString>")]
    #[serde(rename = "Xᵤ")]
    x_u: Option<BigInt>,
    #[serde_as(as = "Option<NumberFromString>")]
    #[serde(rename = "Xᵥ")]
    x_v: Option<BigInt>,
    #[serde_as(as = "Option<NumberFromString>")]
    #[serde(rename = "Yᵤ")]
    y_u: Option<BigInt>,
    #[serde_as(as = "Option<NumberFromString>")]
    #[serde(rename = "Yᵥ")]
    y_v: Option<BigInt>,
}
pub struct Pake<C: EllipticCurve + 'static> {
    // Public variables
    pub pub_pake: PakePubKey,

    // Private variables
    curve: &'static C, // You need to define the EllipticCurve struct
    p: BigInt,
    pw: Vec<u8>,
    vpw_u: Option<BigInt>,
    vpw_v: Option<BigInt>,
    upw_u: Option<BigInt>,
    upw_v: Option<BigInt>,
    a_alpha: Option<[u8; 32]>,
    a_alpha_u: Option<BigInt>,
    a_alpha_v: Option<BigInt>,
    z_u: Option<BigInt>,
    z_v: Option<BigInt>,
    pub k: Option<[u8; 32]>,
}

fn is_valid_point(x: &Option<BigInt>, y: &Option<BigInt>) -> Result<(BigInt, BigInt), Error> {
    match (x, y) {
        (None, Some(_)) | (Some(_), None) | (None, None) => {
            return Err(Error::InvalidPoint {
                x: x.clone(),
                y: y.clone(),
            }
            .into())
        }
        (Some(x), Some(y)) => Ok((x.clone(), y.clone())),
    }
}

fn bigint_to_signed_bytes_be(num: &BigInt) -> Vec<u8> {
    if num == &BigInt::zero() {
        vec![]
    } else {
        num.to_signed_bytes_be()
    }
}

/// Used for pake negotiation
///
/// ```
/// use crypto::pake::{Pake, Role};
/// use serde_json;
/// fn main() {
///     let pake = Pake::new(Role::Sender, None);
///     let str = serde_json::to_string(&pake.pub_pake).unwrap();
///     println!("a{str}");
/// }
///
/// ```
///
///
///
impl Pake<SIEC255Params> {
    pub fn new(role: Role, key: Option<&[u8]>) -> Self {
        let weak_key = key.unwrap_or(&[1u8, 2, 3]);
        let u_u =
            BigInt::parse_bytes(b"793136080485469241208656611513609866400481671853", 10).unwrap();
        let u_v = BigInt::parse_bytes(
            b"18458907634222644275952014841865282643645472623913459400556233196838128612339",
            10,
        )
        .unwrap();
        let v_u =
            BigInt::parse_bytes(b"1086685267857089638167386722555472967068468061489", 10).unwrap();
        let v_y = BigInt::parse_bytes(
            b"19593504966619549205903364028255899745298716108914514072669075231742699650911",
            10,
        )
        .unwrap();

        assert!(siec255().is_on_curve(&u_u, &u_v));
        assert!(siec255().is_on_curve(&v_u, &v_y));
        let curve = siec255();
        let p = curve.p.clone();
        match role {
            Role::Reciever => Pake {
                pub_pake: PakePubKey {
                    role: role,
                    u_u: u_u,
                    u_v: u_v,
                    v_u: v_u,
                    v_v: v_y,
                    x_u: None,
                    x_v: None,
                    y_u: None,
                    y_v: None,
                },
                curve: curve,
                p: p,
                pw: weak_key.to_vec(),
                vpw_u: None,
                vpw_v: None,
                upw_u: None,
                upw_v: None,
                a_alpha: None,
                a_alpha_u: None,
                a_alpha_v: None,
                z_u: None,
                z_v: None,
                k: None,
            },
            Role::Sender => {
                let (v_u_pw, v_v_pw) = curve.scalar_mult(&v_u, &v_y, &weak_key);
                let (u_u_pw, u_v_pw) = curve.scalar_mult(&u_u, &u_v, &weak_key);
                let mut a_alpha = [0u8; 32];
                let mut rng = rand::thread_rng();
                rng.fill_bytes(&mut a_alpha);

                let (a_alpha_u, a_alpha_v) = curve.scalar_base_mult(&a_alpha);
                let (x_u, x_v) = curve.add(&u_u_pw, &u_v_pw, &a_alpha_u, &a_alpha_v); // "X"
                Pake {
                    pub_pake: PakePubKey {
                        role,
                        u_u,
                        u_v,
                        v_u,
                        v_v: v_y,
                        x_u: Some(x_u),
                        x_v: Some(x_v),
                        y_u: None,
                        y_v: None,
                    },
                    curve: curve,
                    p: p,
                    pw: weak_key.to_vec(),
                    vpw_u: Some(v_u_pw),
                    vpw_v: Some(v_v_pw),
                    upw_u: Some(u_u_pw),
                    upw_v: Some(u_v_pw),
                    a_alpha: Some(a_alpha),
                    a_alpha_u: Some(a_alpha_u),
                    a_alpha_v: Some(a_alpha_v),
                    z_u: None,
                    z_v: None,
                    k: None,
                }
            }
        }
    }
    pub fn update(&mut self, key: PakePubKey) -> Result<(), Error> {
        if self.pub_pake.role == key.role {
            return Err(Error::SameRole);
        }
        match self.pub_pake.role {
            Role::Sender => {
                let (y_u, y_v) = is_valid_point(&key.y_u, &key.y_v)?;
                if !self.curve.is_on_curve(&y_u, &y_v) {
                    return Err(Error::PointNotOnCurve { x: y_u, y: y_v });
                }
                let (vpw_u, vpw_v) = is_valid_point(&self.vpw_u, &self.vpw_v)?;
                let (z_u, z_v) =
                    self.curve
                        .add(&y_u, &y_v, &vpw_u, &((-vpw_v).modpow(&1.into(), &self.p)));
                self.z_u = Some(z_u);
                self.z_v = Some(z_v);
                let (mut z_u, mut z_v) = is_valid_point(&self.z_u, &self.z_v)?;

                (z_u, z_v) = if let Some(a_alpha) = self.a_alpha {
                    self.curve.scalar_mult(&z_u, &z_v, &a_alpha)
                } else {
                    return Err(Error::InvalidAlpha);
                };

                let (x_u, x_v) = is_valid_point(&self.pub_pake.x_u, &self.pub_pake.x_v)?;
                let mut hasher = Sha256::new();
                hasher.write_all(self.pw.as_slice()).unwrap();
                hasher.write_all(&bigint_to_signed_bytes_be(&x_u)).unwrap();
                hasher.write_all(&bigint_to_signed_bytes_be(&x_v)).unwrap();
                hasher.write_all(&bigint_to_signed_bytes_be(&y_u)).unwrap();
                hasher.write_all(&bigint_to_signed_bytes_be(&y_v)).unwrap();
                hasher.write_all(&bigint_to_signed_bytes_be(&z_u)).unwrap();
                hasher.write_all(&bigint_to_signed_bytes_be(&z_v)).unwrap();
                self.k = Some(
                    hasher
                        .finalize()
                        .as_slice()
                        .try_into()
                        .expect("Wrong length"),
                );
                debug!("K sender: {:x?}", self.k)
            }
            Role::Reciever => {
                let (x_u, x_v) = is_valid_point(&key.x_u, &key.x_v)?;

                // Make sure X is on the curve
                if !self.curve.is_on_curve(&x_u, &x_v) {
                    return Err(Error::PointNotOnCurve { x: x_u, y: x_v });
                }
                (self.pub_pake.x_u, self.pub_pake.x_v) = (Some(x_u.clone()), Some(x_v.clone()));

                // Compute Y
                let (v_u_pw, v_v_pw) =
                    self.curve
                        .scalar_mult(&self.pub_pake.v_u, &self.pub_pake.v_v, &self.pw);
                let (u_u_pw, u_v_pw) =
                    self.curve
                        .scalar_mult(&self.pub_pake.u_u, &self.pub_pake.u_v, &self.pw);
                (self.vpw_u, self.vpw_v) = (Some(v_u_pw.clone()), Some(v_v_pw.clone()));
                (self.upw_u, self.upw_v) = (Some(u_u_pw.clone()), Some(u_v_pw.clone()));
                let mut a_alpha = [0u8; 32];
                let mut rng = rand::thread_rng();
                rng.fill_bytes(&mut a_alpha);
                self.a_alpha = Some(a_alpha);

                let (a_alpha_u, a_alpha_v) = self.curve.scalar_base_mult(&a_alpha);
                self.a_alpha_u = a_alpha_u.clone().into();
                self.a_alpha_v = a_alpha_v.clone().into();
                let (y_u, y_v) = self.curve.add(&v_u_pw, &v_v_pw, &a_alpha_u, &a_alpha_v);

                // Y point
                (self.pub_pake.y_u, self.pub_pake.y_v) = (Some(y_u.clone()), Some(y_v.clone()));
                let (z_u, z_v) =
                    self.curve
                        .add(&x_u, &x_v, &u_u_pw, &((-u_v_pw).modpow(&1.into(), &self.p)));
                let (z_u, z_v) = self.curve.scalar_mult(&z_u, &z_v, &a_alpha);
                self.z_u = Some(z_u.clone());
                self.z_v = Some(z_v.clone());
                let mut hasher = Sha256::new();
                hasher.write_all(&self.pw.as_slice()).unwrap();
                hasher.write_all(&bigint_to_signed_bytes_be(&x_u)).unwrap();
                hasher.write_all(&bigint_to_signed_bytes_be(&x_v)).unwrap();
                hasher.write_all(&bigint_to_signed_bytes_be(&y_u)).unwrap();
                hasher.write_all(&bigint_to_signed_bytes_be(&y_v)).unwrap();
                hasher.write_all(&bigint_to_signed_bytes_be(&z_u)).unwrap();
                hasher.write_all(&bigint_to_signed_bytes_be(&z_v)).unwrap();
                self.k = Some(
                    hasher
                        .finalize()
                        .as_slice()
                        .try_into()
                        .expect("Wrong length"),
                );
                debug!("K receiver: {:x?}", self.k)
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod test {
    use super::Pake;

    #[test]
    fn test_impl() {
        let _a = Pake::new(super::Role::Sender, None);

        // let msg: Message = Message::Pake {
        //     bytes: base64::engine::general_purpose::STANDARD
        //         .encode(&serde_json::to_string(&a.pub_pake).unwrap()),
        //     bytes2: "hello".to_string(),
        // };
        // let msg_json = serde_json::to_string(&msg).unwrap();
        // println!("{msg_json}");
    }
}
