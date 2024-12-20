/* global bootstrap: false */
(() => {
'use strict'
const tooltipTriggerList = Array.from(document.querySelectorAll('[data-bs-toggle="tooltip"]'))
tooltipTriggerList.forEach(tooltipTriggerEl => {
new bootstrap.Tooltip(tooltipTriggerEl)
})
})()

const pluginCanvasBackgroundColor = {
  id: 'customCanvasBackgroundColor',
  beforeDraw: (chart, args, options) => {
    const {ctx, chartArea: { top, bottom, left, right, width, height },
        scales: {x, y}
    } = chart;
    ctx.save();
    ctx.globalCompositeOperation = 'destination-over';
    ctx.fillStyle = options.color || '#99ffff';
    ctx.fillRect(left, top, width, height);
    ctx.restore();
  }
}

for (let key in objects.datasets) {
    let element = objects.datasets[key];
    if (element instanceof Bar) {
        let h = element;
        console.log('test ' + h.id);
        var ctx = document.getElementById('chart-bar-' + h.id);
        var myChart = new Chart(ctx, {
            type: 'bar',
            data: {
                labels: h.labels,
                datasets: [{
                    label: h.name,
                    data: h.values,
                    borderWidth: 1,
                    backgroundColor: PCOLORS[0],
                    borderColor: '#FFFFFF'
                }]
            },
            options: {
                scales: {
                    y: {
                        title: {
                            display: true,
                            text: h.y_label,
                        },
                        beginAtZero: true,
                        grid: {
                            color: '#FFFFFF',
                        }
                    },
                    x: {
                        title: {
                            display: true,
                            text: h.x_label,
                        },
                        grid: {
                            color: '#FFFFFF',
                        },
                        ticks: {
                            maxRotation: 90,
                            minRotation: 65
                        }
                    },
                },
                plugins: {
                    customCanvasBackgroundColor: {
                        color: '#E5E4EE',
                    }
                }
            },
            plugins: [pluginCanvasBackgroundColor],
        });
        buildPlotDownload(myChart, h.id, fname);
        if (h.log_toggle) {
            buildLogToggle(myChart, "bar-" + h.id);
        }
    } else if (element instanceof MultiBar) {
        let m = element;
        console.log('multi-test ' + m.id);
        var ctx = document.getElementById('chart-bar-' + m.id);
        var myChart = new Chart(ctx, {
            type: 'bar',
            data: {
                labels: m.labels,
                datasets: Array.from(m.values.entries()).reverse().map(function([i, v]) {
                    return {
                        label: m.names[i],
                        data: v,
                        borderWidth: 1,
                        backgroundColor: PCOLORS[i % PCOLORS.length],
                        borderColor: '#FFFFFF'
                    };
                }),
            },
            options: {
                scales: {
                    y: {
                        title: {
                            display: true,
                            text: m.y_label,
                        },
                        beginAtZero: true,
                        grid: {
                            color: '#FFFFFF',
                        },
                        stacked: false,
                    },
                    x: {
                        title: {
                            display: true,
                            text: m.x_label,
                        },
                        grid: {
                            color: '#FFFFFF',
                        },
                        ticks: {
                            maxRotation: 90,
                            minRotation: 65
                        },
                        stacked: true,
                    },
                },
                plugins: {
                    customCanvasBackgroundColor: {
                        color: '#E5E4EE',
                    }
                }
            },
            plugins: [pluginCanvasBackgroundColor],
        });
        buildPlotDownload(myChart, m.id, fname);
        if (m.log_toggle) {
            buildLogToggle(myChart, "bar-" + m.id);
        }
    }
}

console.log("tables");
for (let key in objects.tables) {
    let table = objects.tables[key];
    console.log(table);
    buildTableDownload(table, key, key + '_' + fname);
}

// var tabs = document.querySelectorAll('button[data-bs-toggle="tab"]')
// tabs.forEach(function(tab) {
//     tab.addEventListener('show.bs.tab', function (event) {
//         document.querySelector(event.target.dataset.bsTarget).classList.remove('d-none');
//         document.querySelector(event.relatedTarget.dataset.bsTarget).classList.add('d-none');
//     });
// });
