import numpy as np
from PIL import Image
W,H=1920,1080
def hash1(i, seed=0.0):
    return np.modf(np.abs(np.sin(i*127.1+seed*311.7))*43758.5453)[0]
def noise1(x, seed):
    i=np.floor(x); f=x-i; u=f*f*(3-2*f)
    return hash1(i,seed)*(1-u)+hash1(i+1,seed)*u
def ridged(x, seed, oct=6):
    v=0; a=0.55; fr=1.0
    for k in range(oct):
        n=1-np.abs(2*noise1(x*fr, seed+k*7.3)-1)
        v+=a*n*n; fr*=2.07; a*=0.48
    return v
def fbm1(x, seed, oct=4):
    v=0; a=0.5; fr=1.0
    for k in range(oct):
        v+=a*noise1(x*fr, seed+k*7.3); fr*=2.03; a*=0.5
    return v
def hash2(x,y):
    return np.modf(np.abs(np.sin(x*12.9898+y*78.233))*43758.5453)[0]
def smooth(a,b,x):
    t=np.clip((x-a)/(b-a),0,1); return t*t*(3-2*t)
def mix(a,b,t):
    t=np.asarray(t)
    return a+(np.asarray(b)-a)*(t[...,None] if t.ndim else t)

def scene(snow=0.0, name='lake.png', w=W, h=H):
    ys,xs=np.mgrid[0:h,0:w].astype(np.float64)
    u=xs/w; v=1-ys/h; aspect=w/h
    horizon=0.34
    t=np.clip((v-horizon)/(1-horizon),0,1)
    top=np.array([0.015,0.03,0.08]); mid=np.array([0.04,0.09,0.18]); low=np.array([0.2,0.27,0.36])
    sky=mix(low,mid,smooth(0,0.45,t)); sky=mix(sky,top,smooth(0.35,1.0,t))
    band=np.exp(-((v-0.62-0.3*(u-0.45))/0.07)**2)*(0.4+0.6*fbm1(u*14+v*5,3.0))
    sky=sky+band[...,None]*np.array([0.045,0.05,0.07])
    gx=np.floor(xs/2.5); gy=np.floor(ys/2.5)
    r=hash2(gx,gy)
    star=(r>0.9972)*(0.3+0.7*hash2(gx+1.3,gy+7.7)**2)*smooth(horizon+0.08,horizon+0.3,v)
    sky=sky+star[...,None]*np.array([0.92,0.94,1.0])
    mx,my=0.73,0.79
    d=np.sqrt(((u-mx)*aspect)**2+(v-my)**2)
    sky=sky+(np.exp(-(d/0.16)**2)*0.12+np.exp(-(d/0.05)**2)*0.25)[...,None]*np.array([0.7,0.78,0.92])
    sky=mix(sky,np.array([0.96,0.95,0.9]),smooth(0.03,0.026,d))
    img=sky.copy()
    haze=np.array([0.2,0.26,0.34])
    # Ranges far to near: (height of its foot above the horizon, how tall, colour, how often it peaks, seed, haze share)
    ranges=[(0.10,0.30,np.array([0.17,0.22,0.3]),1.3,1.0,0.55),
            (0.05,0.22,np.array([0.1,0.14,0.2]),2.0,2.0,0.35),
            (0.02,0.14,np.array([0.06,0.085,0.12]),2.8,3.0,0.18),
            (0.0,0.07,np.array([0.03,0.045,0.065]),4.2,4.0,0.06)]
    for k,(foot,tall,col,freq,seed,hz) in enumerate(ranges):
        x=u*freq*3+seed*13
        ridge=horizon+foot+tall*ridged(x,seed)
        inside=(v<ridge)&(v>horizon-0.002)
        # Light from the moon on the right: faces that rise toward it are lit, from a smooth slope.
        slope=ridged(x+0.004,seed,3)-ridged(x-0.004,seed,3)
        lit=smooth(-0.02,0.04,slope*-1 if mx<0.5 else slope)
        # Down from the crest it fades into the haze lying in the valley.
        below=np.clip((ridge-v)/(tall+foot+1e-6),0,1)
        c=col*(0.8+0.45*lit[...,None])
        c=mix(c,haze,smooth(0.1,1.0,below)*hz+hz*0.3)
        # Snow on the heights: always on the far peaks, everywhere high once it snows.
        # Snow lies down from the crest as far as the peak is high, in ragged tongues down its gullies.
        peak=np.clip((ridge-horizon)/0.42,0,1)
        reach=(0.012+0.11*smooth(0.3,0.85,peak))*(0.55+0.9*fbm1(u*55+seed*3,seed+5))*(1.0+1.5*snow)
        down=ridge-v
        cap=smooth(reach,reach*0.6,down)*(smooth(0.35,0.6,peak) if k<2 else 0.0)
        # Once it snows, the lower ranges whiten too, thinly, darker in the forested valleys.
        dusting=snow*(0.85 if k<2 else 0.55)*smooth(0.0,0.02,down)*(0.6+0.4*fbm1(u*9+seed,seed+9))
        cap=np.maximum(cap,snow*smooth(reach*1.6,reach*0.9,down)*(0.8+0.2*fbm1(u*20,seed+9)))
        cap=np.maximum(cap,dusting*smooth(0.0,0.25,1-below))
        cap=np.clip(cap,0,1)
        white=np.array([0.68,0.74,0.84])*(0.62+0.5*lit[...,None])
        white=mix(white,haze,hz*0.5)
        c=mix(c,white,cap)
        img=np.where(inside[...,None],c,img)
    # Pines along the near shore: tiered triangles, taller and shorter, packed close.
    shore=horizon+0.004
    pine=np.zeros((h,w),bool)
    for layer,(spacing,scale) in enumerate([(26,1.0),(17,0.7)]):
        tx=xs/spacing+layer*0.5
        cell=np.floor(tx); fx=tx-cell-0.5
        tall=(0.03+0.05*hash1(cell,5.0+layer))*scale
        hgt=(v-shore)/tall
        tiers=np.modf(np.clip(hgt,0,1)*4)[0]
        half=(1-hgt)*0.42*(0.75+0.5*tiers)
        pine|=(hgt>=0)&(hgt<1)&(np.abs(fx)<half)
    pine|=(v<shore+0.006)&(v>horizon-0.002)
    tree=np.array([0.012,0.02,0.028])
    img=np.where(pine[...,None],tree,img)
    if snow>0:
        # Snow lies along the top of each tier of branches.
        frosted=np.zeros((h,w),bool)
        for layer,(spacing,scale) in enumerate([(26,1.0),(17,0.7)]):
            tx=xs/spacing+layer*0.5
            cell=np.floor(tx); fx=tx-cell-0.5
            tall=(0.03+0.05*hash1(cell,5.0+layer))*scale
            hgt=(v-shore)/tall
            tiers=np.modf(np.clip(hgt,0,1)*4)[0]
            half=(1-hgt)*0.42*(0.75+0.5*tiers)
            frosted|=(hgt>=0)&(hgt<1)&(np.abs(fx)<half)&(tiers<0.28)
        img=np.where((frosted&pine)[...,None],np.array([0.46,0.52,0.6]),img)
    # The lake mirrors everything above, darker, broken into ripples; the moon's path glitters on it.
    water=v<horizon
    depth=np.clip((horizon-v)/horizon,0,1)
    wave=np.sin(ys*(0.6+1.4*depth)*0.5+np.sin(xs*0.011+ys*0.02)*2.5)
    mv=2*horizon-v+wave*0.0018*(0.2+depth*2)
    mu=u+np.sin(ys*0.27+xs*0.003)*0.002*depth
    iy=np.clip(((1-mv)*h).astype(int),0,h-1); ix=np.clip((mu*w).astype(int),0,w-1)
    refl=img[iy,ix]*np.array([0.6,0.66,0.74])
    glitter=np.exp(-((u-mx)*aspect/(0.015+0.08*depth))**2)*np.clip(wave,0,1)**4*smooth(0.0,0.15,depth)
    refl=refl+glitter[...,None]*np.array([0.8,0.82,0.85])
    refl=mix(refl,np.array([0.008,0.016,0.03]),smooth(0.15,1.0,depth)*0.65)
    img=np.where(water[...,None],refl,img)
    mist=np.exp(-((v-horizon-0.008)/0.028)**2)*(0.45+0.55*fbm1(u*5+2,8.0))
    img=mix(img,np.array([0.3,0.37,0.46]),mist*0.4)
    vig=1-0.3*((u-0.5)**2*1.1+(v-0.55)**2*1.4)
    img=img*vig[...,None]+(hash2(xs*1.7,ys*2.3)[...,None]-0.5)*0.01
    Image.fromarray((np.clip(img,0,1)*255).astype(np.uint8)).save(name)
scene(0.0,'lake-night.png'); scene(1.0,'lake-night-snow.png')
