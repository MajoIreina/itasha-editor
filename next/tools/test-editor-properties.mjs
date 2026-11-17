import assert from 'node:assert/strict';
import {parseHex,hexToRgb,rgbToHex,rgbToHsv,hsvToRgb,parseRgbChannel} from '../src/colorMath.ts';
import {resizeProperty} from '../src/layerPropertiesMath.ts';

assert.equal(parseHex(' #AbC '),'#aabbcc');
assert.equal(parseHex('FF0080'),'#ff0080');
for(const bad of ['', '#', '#12', '#1234', '#1234567', '#GG1234', 'rgb(1,2,3)']) assert.equal(parseHex(bad),null);
for(const [text,expected] of [['0',0],['255',255],['256',null],['-1',null],['1.5',null],['1e2',null],['',null]]) assert.equal(parseRgbChannel(text),expected);
for(const rgb of [[255,0,0],[0,255,0],[0,0,255],[255,255,255],[0,0,0],[128,128,128],[23,190,217],[99,3,215]]) {
  assert.deepEqual(hsvToRgb(rgbToHsv(rgb)),rgb);
  assert.deepEqual(hexToRgb(rgbToHex(rgb)),rgb);
}
for(let r=0;r<=255;r+=17)for(let g=0;g<=255;g+=17)for(let b=0;b<=255;b+=17)assert.deepEqual(hsvToRgb(rgbToHsv([r,g,b])),[r,g,b]);
assert.equal(rgbToHsv([0,0,0],211).h,211,'black must retain chosen hue');
assert.deepEqual(hsvToRgb({h:360,s:1,v:1}),[255,0,0]);
assert.deepEqual(hsvToRgb({h:120,s:1,v:.5}),[0,128,0]);
const p={xMm:31,yMm:62,widthMm:300,heightMm:200,rotationDeg:42,aspectRatioLocked:true};
const resized=resizeProperty(p,'widthMm',600);
assert.equal(resized.heightMm,400);assert.equal(resized.xMm,31);assert.equal(resized.yMm,62);assert.equal(resized.rotationDeg,42);
assert.equal(resizeProperty(p,'heightMm',100).widthMm,150);
assert.equal(resizeProperty({...p,aspectRatioLocked:false},'widthMm',600).heightMm,200);
for(const value of [0,10001,Infinity,NaN])assert.equal(resizeProperty(p,'widthMm',value),null);
assert.equal(resizeProperty({...p,widthMm:10000,heightMm:10},'heightMm',11),null,'lock must not exceed other dimension limit');
assert.equal(resizeProperty({...p,widthMm:10000,heightMm:1},'widthMm',9999),null,'lock must not truncate the other axis to 1 mm');
console.log('Color inputs, 4096 RGB/HSV round trips, hue retention and ratio-locked property limits passed.');
