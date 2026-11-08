self.addEventListener('install',e=>e.waitUntil(self.skipWaiting()));
self.addEventListener('activate',e=>e.waitUntil(self.clients.claim()));
self.addEventListener('fetch',event=>{
 const url=new URL(event.request.url);if(!/^https?:$/.test(url.protocol))return;
 if(url.origin===self.location.origin&&(url.pathname.startsWith('/__')||['/','/studio','/sw.js'].includes(url.pathname)))return;
 event.respondWith((async()=>{const client=await self.clients.get(event.clientId),vehicle=client?new URL(client.url).searchParams.get('studioVehicle')||'su7':'su7',body=['GET','HEAD'].includes(event.request.method)?null:await event.request.clone().text();return fetch('/__archive',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({url:event.request.url,method:event.request.method,body,vehicle})});})());
});
