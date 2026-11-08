const { contextBridge } = require('electron');
const argument = process.argv.find(v => v.startsWith('--itasha-host='));
const host = argument ? JSON.parse(decodeURIComponent(argument.slice('--itasha-host='.length))) : {};
contextBridge.exposeInMainWorld('itashaHost', Object.freeze(host));
