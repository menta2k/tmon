/** ECharts on-demand registration. Components import VChart from here so tests can mock it. */
import { use } from 'echarts/core'
import { CanvasRenderer } from 'echarts/renderers'
import { LineChart, PieChart, MapChart } from 'echarts/charts'
import {
  GridComponent,
  LegendComponent,
  TooltipComponent,
  VisualMapComponent,
  DataZoomComponent,
} from 'echarts/components'
import VChart from 'vue-echarts'

use([
  CanvasRenderer,
  LineChart,
  PieChart,
  MapChart,
  GridComponent,
  LegendComponent,
  TooltipComponent,
  VisualMapComponent,
  DataZoomComponent,
])

export { VChart }
