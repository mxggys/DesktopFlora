import sys, numpy as np
from PIL import Image, ImageDraw, ImageFilter
out=sys.argv[1]; W,H=2560,1440
rng=np.random.default_rng(21)
def smooth(a,b,x):
    t=np.clip((x-a)/(b-a),0,1); return t*t*(3-2*t)
ys,xs=np.mgrid[0:H,0:W].astype(np.float64); u=xs/W; v=1-ys/H
# Deep brown at the top, warming to rust and amber low down, as a room lit by a fire out of sight.
top=np.array([0.11,0.05,0.035]); mid=np.array([0.3,0.11,0.06]); low=np.array([0.55,0.24,0.1])
img=top+(mid-top)*smooth(0.95,0.35,v)[...,None]
img=img+(low-mid)*smooth(0.45,0.0,v)[...,None]*0.8
warm=np.exp(-(((u-0.3)*1.4)**2+((v-0.05)*1.6)**2)/0.12)
img=img+warm[...,None]*np.array([0.35,0.15,0.05])
edge=np.sqrt(((u-0.5)*1.2)**2+((v-0.5)*1.3)**2)
img=img*(1-0.45*smooth(0.35,0.95,edge))[...,None]
base=Image.fromarray((np.clip(img,0,1)*255).astype(np.uint8)).convert('RGBA')
# Soft lights far off, out of focus.
bokeh=Image.new('RGBA',(W,H),(0,0,0,0)); bd=ImageDraw.Draw(bokeh)
for i in range(70):
    x=rng.uniform(0,W); y=rng.uniform(H*0.35,H*1.0); r=rng.uniform(14,60)
    c=[(255,170,90),(255,140,70),(255,200,120)][i%3]
    bd.ellipse([x-r,y-r,x+r,y+r],fill=c+(int(rng.uniform(18,55)),),outline=c+(int(rng.uniform(30,70)),),width=2)
base=Image.alpha_composite(base,bokeh.filter(ImageFilter.GaussianBlur(3)))
# Autumn leaves: some far and blurred, a few near, sharp, drifting across.
def leaf(draw,x,y,s,a,col,alpha,lobes):
    pts=[]
    for t in np.linspace(0,2*np.pi,60):
        r=s*(0.5+0.5*abs(np.cos(t)))*(1+0.18*np.cos(lobes*t))**1.0
        px=np.cos(t)*r; py=np.sin(t)*r*0.62
        pts.append((x+px*np.cos(a)-py*np.sin(a),y+px*np.sin(a)+py*np.cos(a)))
    draw.polygon(pts,fill=col+(alpha,))
    for k in (-1,1):
        draw.line([(x,y),(x+s*0.7*np.cos(a+k*0.6),y+s*0.7*np.sin(a+k*0.6))],fill=(60,24,10,alpha//3),width=2)
    draw.line([(x-s*1.25*np.cos(a),y-s*1.25*np.sin(a)),(x+s*np.cos(a),y+s*np.sin(a))],fill=(60,24,10,alpha//2),width=2)
cols=[(178,64,26),(205,112,34),(150,40,22),(214,150,52),(128,58,24),(190,86,30)]
for layer,(count,size,blur,alpha) in enumerate([(40,(14,26),6,120),(26,(24,40),2.5,200),(9,(46,80),0,245)]):
    lv=Image.new('RGBA',(W,H),(0,0,0,0)); ld=ImageDraw.Draw(lv)
    for i in range(count):
        leaf(ld,rng.uniform(0,W),rng.uniform(0,H),rng.uniform(*size),rng.uniform(0,np.pi),cols[(i+layer)%6],alpha,rng.choice([3,5,7]))
    if blur: lv=lv.filter(ImageFilter.GaussianBlur(blur))
    base=Image.alpha_composite(base,lv)
# A drift of fallen leaves along the bottom.
lv=Image.new('RGBA',(W,H),(0,0,0,0)); ld=ImageDraw.Draw(lv)
for i in range(260):
    x=rng.uniform(-30,W+30); y=H-rng.uniform(0,90)**1.1*0.9-10
    leaf(ld,x,y,rng.uniform(16,34),rng.uniform(-0.6,0.6),cols[i%6],int(rng.uniform(200,250)),rng.choice([3,5,7]))
base=Image.alpha_composite(base,lv.filter(ImageFilter.GaussianBlur(0.6)))
arr=np.asarray(base.convert('RGB')).astype(np.float64)/255
arr=arr+(np.asarray(Image.effect_noise((W,H),12)).astype(np.float64)[...,None]/255-0.5)*0.02
Image.fromarray((np.clip(arr,0,1)*255).astype(np.uint8)).save(out)
