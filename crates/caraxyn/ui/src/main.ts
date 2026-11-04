import { mount } from 'svelte';
import '@fontsource/roboto/400.css';
import '@fontsource/roboto/500.css';
import '@fontsource/roboto/700.css';
import './app.css';
import App from './App.svelte';

mount(App, { target: document.getElementById('app')! });
