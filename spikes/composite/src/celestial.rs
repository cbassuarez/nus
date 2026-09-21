//! Low-precision Sun/Moon ephemerides. Orbital elements and leading lunar
//! perturbations: Paul Schlyter, https://stjarnhimlen.se/comp/ppcomp.html.
//! Decorative naked-eye sky, not an ephemeris for navigation: geocentric Moon,
//! no topocentric parallax, refraction, nutation or aberration.
const D:f64=std::f64::consts::PI/180.0;
#[derive(Clone,Copy,Debug)]
pub struct Body {pub ra:f64,pub dec:f64,pub alt:f64,pub az:f64}
#[derive(Clone,Copy,Debug)]
pub struct Sky {pub sun:Body,pub moon:Body,pub illumination:f64,pub waxing:bool}
fn kepler(m:f64,e:f64)->(f64,f64) {
    let m=m.rem_euclid(360.0)*D;let mut eccentric=m;
    for _ in 0..5 {eccentric-=(eccentric-e*eccentric.sin()-m)/(1.0-e*eccentric.cos());}
    let x=eccentric.cos()-e;let y=(1.0-e*e).sqrt()*eccentric.sin();
    (y.atan2(x),x.hypot(y))
}
fn body(lon:f64,lat:f64,obl:f64,lst:f64,phi:f64)->Body {
    let x=lon.cos()*lat.cos();let y=lon.sin()*lat.cos();let z=lat.sin();
    let ye=y*obl.cos()-z*obl.sin();let ze=y*obl.sin()+z*obl.cos();
    let ra=ye.atan2(x);let dec=ze.atan2(x.hypot(ye));let hour=lst-ra;
    let alt=(phi.sin()*dec.sin()+phi.cos()*dec.cos()*hour.cos()).clamp(-1.0,1.0).asin();
    let az=(-dec.cos()*hour.sin()).atan2(dec.sin()*phi.cos()-dec.cos()*phi.sin()*hour.cos());
    Body{ra:ra.rem_euclid(std::f64::consts::TAU)/D/15.0,dec:dec/D,alt,az}
}
pub fn sky(ms:f64,latitude:f64,longitude:f64)->Sky {
    let d=ms/86400000.0-10956.0; // 1999-12-31 00:00 UTC
    let obl=(23.4393-3.563e-7*d)*D;
    let sm=356.0470+0.9856002585*d;let sw=282.9404+4.70935e-5*d;
    let (sv,_)=kepler(sm,0.016709-1.151e-9*d);let sl=sv+sw*D;
    let node=(125.1228-0.0529538083*d)*D;let inc=5.1454*D;
    let mw=318.0634+0.1643573223*d;let mm=115.3654+13.0649929509*d;
    let (mv,_)=kepler(mm,0.0549);let v=mv+mw*D;
    let x=node.cos()*v.cos()-node.sin()*v.sin()*inc.cos();
    let y=node.sin()*v.cos()+node.cos()*v.sin()*inc.cos();let z=v.sin()*inc.sin();
    let elong=(mm+mw+node/D-sm-sw)*D;let argument=(mm+mw)*D;
    // Leading perturbations keep the lunar orbit useful at this visual scale.
    let ml=y.atan2(x)+(-1.274*(mm*D-2.0*elong).sin()+0.658*(2.0*elong).sin()-0.186*(sm*D).sin())*D;
    let mb=z.atan2(x.hypot(y))-0.173*(argument-2.0*elong).sin()*D;
    let lst=(280.46061837+360.98564736629*(d-1.5)+longitude).rem_euclid(360.0)*D;
    let phi=latitude.clamp(-90.0,90.0)*D;
    Sky{sun:body(sl,0.0,obl,lst,phi),moon:body(ml,mb,obl,lst,phi),illumination:(1.0-mb.cos()*(ml-sl).cos())/2.0,waxing:(ml-sl).rem_euclid(std::f64::consts::TAU)<std::f64::consts::PI}
}
#[cfg(test)]mod tests {
    use super::*;
    #[test]fn eclipse_and_full_moon(){
        // 2024-04-08 18:00 UTC solar eclipse; 2024-03-25 07:00 UTC full Moon.
        assert!(sky(1712599200000.0,0.0,0.0).illumination<0.005);
        assert!(sky(1711350000000.0,0.0,0.0).illumination>0.995);
    }
    #[test]fn june_solstice_and_poles(){
        let s=sky(1718928000000.0,90.0,0.0);
        assert!((s.sun.dec-23.44).abs()<0.1);assert!((s.sun.alt/D-23.44).abs()<0.1);
        assert!(sky(1718928000000.0,-90.0,0.0).sun.alt<0.0);
        for lat in [-90.0,-33.9,0.0,45.0,90.0]{let s=sky(1718928000000.0,lat,151.2);assert!(s.moon.az.is_finite()&&s.moon.alt.is_finite());}
    }
}
