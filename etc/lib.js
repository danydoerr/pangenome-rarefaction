/*!
  * Panacus JS library
  */

const PCOLORS = ['#f77189', '#bb9832', '#50b131', '#36ada4', '#3ba3ec', '#e866f4'];

class Bar {
    constructor(id, name, x_label, y_label, labels, values, log_toggle) {
        this.id = id;
        this.name = name;
        this.x_label = x_label;
        this.y_label = y_label;
        this.labels = labels;
        this.values = values;
        this.log_toggle = log_toggle;
    }
}

class MultiBar {
    constructor(id, names, x_label, y_label, labels, values, log_toggle) {
        this.id = id;
        this.names = names;
        this.x_label = x_label;
        this.y_label = y_label;
        this.labels = labels;
        this.values = values;
        this.log_toggle = log_toggle;
    }
}

function buildPlotDownload(chart, obj, prefix) {
    console.log('btn-download-plot-' + obj);
    document.getElementById('btn-download-plot-' + obj).onclick = function() {
        var a = document.createElement('a');
        a.href = chart.toBase64Image();
        a.download = prefix + '_' + obj + '.png';
        a.click();
    };
}

function buildTableDownload(table, id, prefix) {
    console.log('btn-download-table-' + id);
    document.getElementById('btn-download-table-' + id).onclick = function() {
        let blob = new Blob([table], {type: 'text/plain'});
        var a = document.createElement('a');
        a.href = URL.createObjectURL(blob);
        a.download = prefix + '_table.tsv';
        a.click();
    };
}

function buildLogToggle(chart, name) {
    document.getElementById('btn-logscale-plot-' + name).addEventListener('change', function(event) {
        if (event.currentTarget.checked) {
            chart.options.scales.y.type = 'logarithmic';
        } else {
            chart.options.scales.y.type = 'linear';
        }
        chart.update();
    });
}
