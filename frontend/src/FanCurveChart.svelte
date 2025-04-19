<script lang="ts">
    import { onMount } from 'svelte';
    import Chart from 'chart.js/auto';
  
    export let fanCurve = [];
  
    let canvas: HTMLCanvasElement;
    let chart: Chart;
  
    onMount(() => {
      const ctx = canvas.getContext('2d');
  
      const data = {
        labels: fanCurve.map(point => `${point.temp}°C`),
        datasets: [
          {
            label: 'Fan Curve',
            data: fanCurve.map(point => point.pwm),
            fill: false,
            borderColor: 'rgb(59, 130, 246)',
            tension: 0.3
          }
        ]
      };
  
      chart = new Chart(ctx, {
        type: 'line',
        data,
        options: {
          responsive: true,
          scales: {
            y: {
              beginAtZero: true,
              max: 100,
              title: {
                display: true,
                text: 'PWM (%)'
              }
            },
            x: {
              title: {
                display: true,
                text: 'Temperature (°C)'
              }
            }
          }
        }
      });
    });
  </script>
  
  <canvas bind:this={canvas} class="w-full max-w-3xl mx-auto bg-gray-900 rounded-lg p-4" />
  